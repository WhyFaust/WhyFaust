from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import Counter
from datetime import date, datetime, timedelta, timezone
from html.parser import HTMLParser
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
LANGUAGE_COLORS = {
    "Python": "#e3b341",
    "JavaScript": "#f1e05a",
    "TypeScript": "#3178c6",
    "C#": "#a178df",
    "SourcePawn": "#ed9569",
    "HTML": "#e34c26",
    "CSS": "#9b7edb",
    "Shell": "#89b97e",
    "Rust": "#dea584",
    "Go": "#00add8",
}


class ProfileDataError(ValueError):
    pass


def text(value: object, field: str, nullable: bool = False) -> str:
    if value is None and nullable:
        return ""
    if not isinstance(value, str):
        raise ProfileDataError(f"Expected text for {field}")
    return value


def count(value: object, field: str) -> int:
    if type(value) is not int or value < 0:
        raise ProfileDataError(f"Expected a nonnegative integer for {field}")
    return value


def required_value(payload: dict, key: str, context: str) -> object:
    if key not in payload:
        raise ProfileDataError(f"GitHub {context} field {key} is missing")
    return payload[key]


def request_text(url: str, *, api: bool = False) -> str:
    headers = {"User-Agent": "WhyFaust-profile", "Accept-Language": "en-US"}
    if api:
        headers["Accept"] = "application/vnd.github+json"
        headers["X-GitHub-Api-Version"] = "2022-11-28"
        token = os.environ.get("GITHUB_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
    else:
        headers["Accept"] = "text/html"
    try:
        with urlopen(Request(url, headers=headers), timeout=30) as response:
            return response.read().decode("utf-8")
    except HTTPError as error:
        raise RuntimeError(f"GitHub request failed with HTTP {error.code}") from None
    except (URLError, TimeoutError, UnicodeError):
        raise RuntimeError("GitHub response could not be retrieved or decoded") from None


def api_json(path: str) -> object:
    try:
        return json.loads(request_text(f"https://api.github.com{path}", api=True))
    except json.JSONDecodeError:
        raise ProfileDataError("GitHub API returned invalid JSON") from None


def get_repositories(login: str) -> list[dict]:
    repositories = {}
    page = 1
    while True:
        query = urlencode({"type": "owner", "sort": "full_name", "direction": "asc", "per_page": 100, "page": page})
        payload = api_json(f"/users/{login}/repos?{query}")
        if not isinstance(payload, list):
            raise ProfileDataError("GitHub repositories response is not a list")
        for repository in payload:
            if not isinstance(repository, dict):
                raise ProfileDataError("GitHub repository is not an object")
            owner = repository.get("owner")
            if not isinstance(owner, dict):
                raise ProfileDataError("GitHub repository owner is missing")
            if type(repository.get("private")) is not bool or repository.get("visibility") not in {"public", "private", "internal"}:
                raise ProfileDataError("GitHub repository visibility is missing")
            if repository["private"] or repository["visibility"] != "public":
                continue
            if text(owner.get("login"), "repository owner").casefold() != login.casefold():
                continue
            name = text(repository.get("name"), "repository name")
            if name in repositories:
                raise ProfileDataError("GitHub repository pagination returned duplicate entries")
            repositories[name] = repository
        if len(payload) < 100:
            return list(repositories.values())
        page += 1


class ContributionParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.cells: dict[str, dict] = {}
        self.tooltips: dict[str, str] = {}
        self.tooltip_target = ""
        self.tooltip_parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if "data-date" in values and "ContributionCalendar-day" in (values.get("class") or "").split():
            identifier = values.get("id")
            day = values.get("data-date")
            level = values.get("data-level")
            if not identifier or not day or not level or not level.isdigit():
                raise ProfileDataError("GitHub calendar cell is incomplete")
            if identifier in self.cells:
                raise ProfileDataError("GitHub calendar contains duplicate cells")
            self.cells[identifier] = {"date": day, "level": int(level)}
        if tag == "tool-tip":
            self.tooltip_target = values.get("for") or ""
            self.tooltip_parts = []

    def handle_data(self, data: str) -> None:
        if self.tooltip_target:
            self.tooltip_parts.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "tool-tip" and self.tooltip_target:
            self.tooltips[self.tooltip_target] = " ".join(self.tooltip_parts).strip()
            self.tooltip_target = ""
            self.tooltip_parts = []


def parse_contributions(document: str, start: date, end: date) -> dict:
    parser = ContributionParser()
    parser.feed(document)
    days = {}
    for identifier, cell in parser.cells.items():
        try:
            current_date = date.fromisoformat(cell["date"])
        except ValueError:
            raise ProfileDataError("GitHub calendar contains an invalid date") from None
        if current_date < start or current_date > end:
            continue
        match = re.match(r"^(No|[0-9][0-9,]*) contributions?\b", parser.tooltips.get(identifier, ""))
        if not match:
            raise ProfileDataError(f"GitHub calendar count missing for {current_date}")
        amount = 0 if match[1] == "No" else int(match[1].replace(",", ""))
        level = cell["level"]
        if not 0 <= level <= 4 or (amount == 0) != (level == 0):
            raise ProfileDataError(f"GitHub calendar intensity mismatch for {current_date}")
        if current_date in days:
            raise ProfileDataError("GitHub calendar contains duplicate dates")
        days[current_date] = {"date": current_date.isoformat(), "count": amount, "level": level}
    expected = {start + timedelta(days=offset) for offset in range((end - start).days + 1)}
    if set(days) != expected:
        raise ProfileDataError("GitHub calendar is incomplete; previous published data retained")
    ordered = [days[key] for key in sorted(days)]
    return {"total": sum(day["count"] for day in ordered), "start": start.isoformat(), "end": end.isoformat(), "days": ordered}


class PinnedRepositoryParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.headings: list[str] = []
        self.heading_parts: list[str] | None = None
        self.div_depth = 0
        self.section_depth: int | None = None
        self.card_depth: int | None = None
        self.names: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if tag == "h2" and self.section_depth is not None:
            self.heading_parts = []
        if tag == "div":
            self.div_depth += 1
            classes = (values.get("class") or "").split()
            if "js-pinned-items-reorder-container" in classes:
                self.section_depth = self.div_depth
            if self.section_depth is not None and "pinned-item-list-item" in classes:
                self.card_depth = self.div_depth
        if tag == "a" and self.card_depth is not None:
            href = values.get("href") or ""
            if re.fullmatch(r"/[A-Za-z0-9-]+/[A-Za-z0-9_.-]+", href) and href[1:] not in self.names:
                self.names.append(href[1:])

    def handle_data(self, data: str) -> None:
        if self.heading_parts is not None:
            self.heading_parts.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "h2" and self.heading_parts is not None:
            self.headings.append(" ".join(" ".join(self.heading_parts).split()))
            self.heading_parts = None
        if tag == "div":
            if self.card_depth == self.div_depth:
                self.card_depth = None
            if self.section_depth == self.div_depth:
                self.section_depth = None
            self.div_depth -= 1


def parse_pins(document: str, login: str) -> list[str]:
    parser = PinnedRepositoryParser()
    parser.feed(document)
    if not any(heading.startswith(("Pinned", "Popular repositories")) for heading in parser.headings):
        raise ProfileDataError("GitHub profile repository section is missing")
    if not any(heading.startswith("Pinned") for heading in parser.headings):
        return []
    return [name.split("/", 1)[1] for name in parser.names if name.split("/", 1)[0].casefold() == login.casefold()]


def normalize_repositories(repositories: list[dict], login: str, pins: list[str]) -> tuple[dict, list[dict], list[dict]]:
    excluded = {login.casefold(), "github-profile-summary-cards"}
    originals = []
    for repository in repositories:
        if any(type(repository.get(field)) is not bool for field in ("fork", "archived", "disabled")):
            raise ProfileDataError("GitHub repository status is missing")
        if repository["fork"] or repository["name"].casefold() in excluded:
            continue
        for field in ("description", "language", "pushed_at"):
            text(required_value(repository, field, "repository"), field, nullable=True)
        originals.append(repository)
    stats = {
        "original_repos": len(originals),
        "total_stars": sum(count(repository.get("stargazers_count"), "repository stars") for repository in originals),
        "total_forks": sum(count(repository.get("forks_count"), "repository forks") for repository in originals),
    }
    language_counts = Counter(text(repository.get("language"), "repository language", nullable=True) for repository in originals)
    languages = [
        {"name": name, "repo_count": amount, "color": LANGUAGE_COLORS.get(name, "#a8afb8")}
        for name, amount in sorted(language_counts.items(), key=lambda item: (-item[1], item[0])) if name
    ]
    candidates = [repository for repository in originals if not repository["archived"] and not repository.get("disabled", False) and repository.get("pushed_at")]
    candidates.sort(key=lambda repository: (text(repository.get("pushed_at"), "repository push date", nullable=True), repository["name"]), reverse=True)
    pinned_order = {name.casefold(): index for index, name in enumerate(pins)}
    candidates.sort(key=lambda repository: pinned_order.get(repository["name"].casefold(), len(pins)))
    projects = [
        {
            "name": repository["name"],
            "description": text(repository.get("description"), "repository description", nullable=True),
            "url": f"https://github.com/{login}/{repository['name']}",
            "stars": count(repository.get("stargazers_count"), "repository stars"),
            "forks": count(repository.get("forks_count"), "repository forks"),
            "language": text(repository.get("language"), "repository language", nullable=True),
            "pushed_at": text(repository.get("pushed_at"), "repository push date", nullable=True),
        }
        for repository in candidates[:4]
    ]
    return stats, languages, projects


def collect_profile(username: str, today: date | None = None) -> dict:
    if not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?", username):
        raise ProfileDataError("Invalid GitHub username")
    payload = api_json(f"/users/{username}")
    if not isinstance(payload, dict) or payload.get("type") != "User":
        raise ProfileDataError("Expected a GitHub user profile")
    login = text(payload.get("login"), "profile login")
    if login.casefold() != username.casefold():
        raise ProfileDataError("GitHub returned a different account")
    profile = {
        "login": login,
        "name": text(required_value(payload, "name", "profile"), "profile name", nullable=True) or login,
        "bio": text(required_value(payload, "bio", "profile"), "profile bio", nullable=True),
        "location": text(required_value(payload, "location", "profile"), "profile location", nullable=True),
        "url": f"https://github.com/{login}",
        "created_at": text(payload.get("created_at"), "profile creation date"),
        "followers": count(payload.get("followers"), "followers"),
        "following": count(payload.get("following"), "following"),
        "public_repos": count(payload.get("public_repos"), "public repositories"),
    }
    repositories = get_repositories(login)
    if len(repositories) != profile["public_repos"]:
        raise ProfileDataError("GitHub public repository listing is incomplete or changed during refresh")
    pins = parse_pins(request_text(profile["url"]), login)
    stats, languages, projects = normalize_repositories(repositories, login, pins)
    end = today or datetime.now(timezone.utc).date()
    start = end - timedelta(days=364)
    calendar = request_text(f"https://github.com/users/{login}/contributions")
    contributions = parse_contributions(calendar, start, end)
    return {"profile": profile, "stats": stats, "languages": languages, "projects": projects, "contributions": contributions}


def snapshot_with_timestamp(data: dict, previous: dict | None, now: datetime | None = None) -> dict:
    previous_payload = {key: value for key, value in (previous or {}).items() if key != "updated_at"}
    if data == previous_payload and previous and isinstance(previous.get("updated_at"), str):
        timestamp = previous["updated_at"]
    else:
        timestamp = (now or datetime.now(timezone.utc)).astimezone(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    return {**data, "updated_at": timestamp}


def main() -> None:
    parser = argparse.ArgumentParser(description="Refresh the public GitHub profile portfolio")
    parser.add_argument("--username", default=os.environ.get("GITHUB_REPOSITORY_OWNER", "WhyFaust"))
    parser.add_argument("--fetch-only", action="store_true")
    arguments = parser.parse_args()
    destination = ROOT / "data" / "profile.json"
    previous = json.loads(destination.read_text(encoding="utf-8")) if destination.exists() else None
    if previous is not None and not isinstance(previous, dict):
        raise ProfileDataError("Existing snapshot is not an object")
    data = snapshot_with_timestamp(collect_profile(arguments.username), previous)
    if not arguments.fetch_only:
        from render_profile import render

        render(data, ROOT)
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(destination)
    print(f"Profile refreshed: {data['profile']['login']}, {data['stats']['original_repos']} own projects, {len(data['contributions']['days'])} calendar days")


if __name__ == "__main__":
    try:
        main()
    except (ProfileDataError, RuntimeError, json.JSONDecodeError) as error:
        print(f"Profile refresh failed: {error}", file=sys.stderr)
        sys.exit(1)
