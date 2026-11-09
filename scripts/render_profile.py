from __future__ import annotations

import base64
import html
import json
import re
import textwrap
from datetime import date, datetime, timedelta, timezone
from pathlib import Path


THEMES = {
    "dark": {
        "background": "#171513",
        "surface": "#24201c",
        "border": "#43382f",
        "text": "#fff5e8",
        "muted": "#c5b2a3",
        "accent": "#f0a17b",
        "accent_text": "#281810",
        "soft": "#a6bd9b",
        "levels": ["#302923", "#674234", "#a45b3e", "#db9164", "#facb95"],
    },
    "light": {
        "background": "#fffaf2",
        "surface": "#f4eadd",
        "border": "#dfcdbb",
        "text": "#34271f",
        "muted": "#796252",
        "accent": "#a64a2c",
        "accent_text": "#fffaf2",
        "soft": "#58774c",
        "levels": ["#eee2d3", "#edc6ae", "#d9936c", "#bc6840", "#874126"],
    },
}

MONO = "ui-monospace, SFMono-Regular, Consolas, Liberation Mono, monospace"
SANS = "-apple-system, BlinkMacSystemFont, Segoe UI, Arial, sans-serif"
INTRODUCTION = "QA automation engineer. Python, integrations and developer tools."


def clean(value: object) -> str:
    text = str(value if value is not None else "")
    return re.sub(r"[\x00-\x08\x0b\x0c\x0e-\x1f\ud800-\udfff]", "", text)


def esc(value: object) -> str:
    return html.escape(clean(value), quote=True)


def short(value: object, width: int) -> str:
    text = " ".join(clean(value).split())
    return text if len(text) <= width else text[: width - 1].rstrip() + "…"


def lines(value: object, width: int, count: int) -> list[str]:
    text = " ".join(clean(value).split())
    wrapped = textwrap.wrap(text, width=width, break_long_words=True, break_on_hyphens=False)
    result = wrapped[:count]
    if len(wrapped) > count:
        result[-1] = short(result[-1], width - 1).rstrip("…") + "…"
    return result


def svg_text(
    x: float,
    y: float,
    value: object,
    size: int,
    color: str,
    weight: int = 400,
    mono: bool = False,
    anchor: str = "start",
) -> str:
    return (
        f'<text x="{x:g}" y="{y:g}" fill="{color}" font-size="{size}" '
        f'font-family="{MONO if mono else SANS}" font-weight="{weight}" '
        f'text-anchor="{anchor}">{esc(value)}</text>'
    )


def rect(x: float, y: float, width: float, height: float, fill: str, radius: float = 0, stroke: str | None = None) -> str:
    return (
        f'<rect x="{x:g}" y="{y:g}" width="{width:g}" height="{height:g}" '
        f'rx="{radius:g}" fill="{fill}"'
        + (f' stroke="{stroke}"' if stroke else "")
        + "/>"
    )


def document(width: int, height: int, title: str, description: str, body: list[str], theme: dict) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img" aria-labelledby="title description">\n'
        f'<title id="title">{esc(title)}</title><desc id="description">{esc(description)}</desc>\n'
        '<style>@media (prefers-reduced-motion: reduce){.motion{display:none}}</style>\n'
        + rect(1, 1, width - 2, height - 2, theme["background"], 22, theme["border"])
        + "\n"
        + "\n".join(body)
        + "\n</svg>\n"
    )


def image_uri(root: Path) -> str:
    image = root / "assets" / "mascot.jpg"
    mime = "image/jpeg"
    if not image.is_file():
        image = root / "assets" / "mascot.png"
        mime = "image/png"
    encoded = base64.b64encode(image.read_bytes()).decode("ascii")
    return f"data:{mime};base64,{encoded}"


def count(value: int) -> str:
    return f"{int(value):,}"


def iso_day(value: object) -> str:
    return date.fromisoformat(clean(value)[:10]).isoformat()


def display_day(value: object) -> str:
    parsed = date.fromisoformat(iso_day(value))
    return f"{parsed.day} {parsed:%b %Y}"


def hero(data: dict, image: str, theme: dict, mobile: bool = False) -> str:
    profile = data["profile"]
    stats = data["stats"]
    login = profile["login"]
    name = profile.get("name") or login
    biography = profile.get("bio") or INTRODUCTION
    created = date.fromisoformat(profile["created_at"][:10])
    year_label = f"On GitHub since {created:%B %Y}"
    location = profile.get("location")
    if location:
        year_label = f"{short(location, 22)}  ·  Since {created.year}"
    width, height = (720, 1130) if mobile else (1200, 596)
    image_x, image_y, image_size = (90, 86, 540) if mobile else (28, 80, 430)
    body = [
        f'<path d="M1 54H{width - 1}" stroke="{theme["border"]}"/>',
        f'<circle cx="27" cy="28" r="5" fill="{theme["accent"]}"/>',
        f'<circle cx="46" cy="28" r="5" fill="{theme["muted"]}" opacity=".65"/>',
        f'<circle cx="65" cy="28" r="5" fill="{theme["soft"]}"/>',
        svg_text(90, 34, f"{login.lower()}@github: ~", 16, theme["muted"], mono=True),
        svg_text(width - 26, 34, "PERSONAL WORKSPACE", 12 if mobile else 14, theme["muted"], mono=True, anchor="end"),
        f'<defs><clipPath id="portrait"><rect x="{image_x}" y="{image_y}" width="{image_size}" height="{image_size}" rx="18"/></clipPath></defs>',
        f'<image x="{image_x}" y="{image_y}" width="{image_size}" height="{image_size}" href="{image}" clip-path="url(#portrait)"/>',
    ]
    if mobile:
        body += [
            svg_text(48, 682, "$ whoami", 22, theme["accent"], mono=True),
            svg_text(48, 754, short(name, 16), 66, theme["text"], 750),
            svg_text(48, 795, f"@{login}", 28, theme["accent"], mono=True),
        ]
        for i, line in enumerate(lines(biography, 39, 2)):
            body.append(svg_text(48, 842 + i * 34, line, 28, theme["text"]))
        body.append(svg_text(48, 913, year_label, 23, theme["muted"]))
        stat_x, stat_y, stat_width, stat_height, gap = 48, 942, 194, 111, 21
    else:
        body += [
            svg_text(506, 109, "$ whoami", 19, theme["accent"], mono=True),
            svg_text(502, 181, short(name, 16), 68, theme["text"], 750),
            svg_text(506, 220, f"@{login}", 22, theme["accent"], mono=True),
            svg_text(41, 554, "CODE  /  COFFEE  /  CURIOSITY", 17, theme["muted"], mono=True),
        ]
        for i, line in enumerate(lines(biography, 52, 3)):
            body.append(svg_text(506, 262 + i * 29, line, 22, theme["text"]))
        body.append(svg_text(506, 355, year_label, 18, theme["muted"]))
        stat_x, stat_y, stat_width, stat_height, gap = 506, 384, 206, 102, 14
    values = [
        (stats["original_repos"], "original repos"),
        (stats["total_stars"], "project stars"),
        (profile["followers"], "followers"),
    ]
    for index, (number, label) in enumerate(values):
        x = stat_x + index * (stat_width + gap)
        body.append(rect(x, stat_y, stat_width, stat_height, theme["surface"], 12))
        body.append(svg_text(x + 18, stat_y + 48, count(number), 40 if mobile else 36, theme["text"], 650))
        body.append(svg_text(x + 18, stat_y + 81, label, 22 if mobile else 16, theme["muted"]))
    language_items = data.get("languages", [])[:3]
    language_text = "  ·  ".join(f'{item["name"]} {item["repo_count"]}' for item in language_items)
    if not language_text:
        language_text = "New projects are on the way."
    if mobile:
        body.append(svg_text(48, 1096, short(language_text, 45), 22, theme["accent"], mono=True))
    else:
        body.append(svg_text(506, 526, "PRIMARY LANGUAGES / REPO COUNT", 13, theme["muted"], mono=True))
        body.append(svg_text(506, 557, short(language_text, 49), 18, theme["accent"], mono=True))
    description = (
        f'{name}, @{login}. {biography} {stats["original_repos"]} original public repositories '
        f'excluding this profile; {stats["total_stars"]} stars on those repositories; '
        f'{profile["followers"]} followers. Languages show primary-language repository counts.'
    )
    return document(width, height, f"{name} | GitHub portfolio", description, body, theme)


def button(label: str, detail: str, theme: dict, primary: bool, mobile: bool = False) -> str:
    width, height = (340, 96) if mobile else (580, 92)
    foreground = theme["accent_text"] if primary else theme["text"]
    background = theme["accent"] if primary else theme["surface"]
    body = [rect(1, 1, width - 2, height - 2, background, 16)]
    if mobile:
        body.append(svg_text(26, 59, label, 29, foreground, 600))
        body.append(svg_text(width - 27, 59, "↗", 30, foreground, anchor="end"))
    else:
        body.append(svg_text(28, 41, label, 25, foreground, 600))
        body.append(svg_text(28, 69, detail, 17, foreground))
        body.append(svg_text(width - 30, 58, "↗", 32, foreground, anchor="end"))
    return document(width, height, label, detail, body, theme)


def project_card(project: dict, index: int, theme: dict, mobile: bool = False) -> str:
    width, height = (720, 262) if mobile else (1200, 190)
    body = []
    name = project["name"]
    description = project.get("description") or "Explore the source, documentation and latest changes on GitHub."
    if mobile:
        body += [
            svg_text(28, 38, f"PROJECT / {index:02}", 17, theme["accent"], mono=True),
            svg_text(28, 83, short(name, 34), 32, theme["text"], 650),
            svg_text(688, 82, "↗", 30, theme["accent"], anchor="end"),
        ]
        for i, line in enumerate(lines(description, 40, 2)):
            body.append(svg_text(28, 128 + i * 36, line, 28, theme["muted"]))
        metadata = f'{project.get("language") or "Mixed"}  ·  {count(project["stars"])} stars'
        body.append(svg_text(28, 215, short(metadata, 43), 24, theme["accent"], mono=True))
        body.append(svg_text(28, 242, f'Pushed {iso_day(project["pushed_at"])}', 20, theme["muted"]))
    else:
        body += [
            svg_text(28, 56, f"{index:02}", 19, theme["accent"], mono=True),
            svg_text(84, 56, short(name, 48), 31, theme["text"], 650),
            svg_text(1168, 55, "↗", 29, theme["accent"], anchor="end"),
        ]
        for i, line in enumerate(lines(description, 86, 2)):
            body.append(svg_text(84, 98 + i * 27, line, 22, theme["muted"]))
        metadata = (
            f'{project.get("language") or "Mixed"}  ·  {count(project["stars"])} stars'
            f'  ·  {count(project["forks"])} forks'
        )
        body.append(svg_text(84, 162, short(metadata, 78), 17, theme["accent"], mono=True))
        body.append(svg_text(1168, 162, f'Pushed {iso_day(project["pushed_at"])}', 16, theme["muted"], anchor="end"))
    return document(width, height, name, f'{description} {metadata}.', body, theme)


def activity(data: dict, theme: dict, mobile: bool = False) -> str:
    contribution_data = data["contributions"]
    days = {date.fromisoformat(day["date"]): day for day in contribution_data["days"]}
    end = date.fromisoformat(contribution_data["end"])
    first = date.fromisoformat(contribution_data["start"])
    last_week = end - timedelta(days=(end.weekday() + 1) % 7)
    start_week = last_week - timedelta(weeks=12) if mobile else first - timedelta(days=(first.weekday() + 1) % 7)
    weeks = (last_week - start_week).days // 7 + 1
    width, height = (720, 480) if mobile else (1200, 340)
    gap = 6 if mobile else 4
    cell = 32 if mobile else min(16, (1060 - (weeks - 1) * gap) / weeks)
    step = cell + gap
    grid_x, grid_y = (111, 161) if mobile else (69, 125)
    total = contribution_data["total"]
    body = [svg_text(28, 48, "GitHub activity", 30 if mobile else 28, theme["text"], 650)]
    if mobile:
        body += [
            svg_text(28, 89, f"{count(total)} contributions over 365 days", 24, theme["accent"]),
            svg_text(28, 124, "A closer look at the last 13 weeks", 20, theme["muted"]),
        ]
    else:
        body += [
            svg_text(28, 81, f'{display_day(first)} — {display_day(end)}', 18, theme["muted"]),
            svg_text(1171, 48, count(total), 38, theme["accent"], 650, anchor="end"),
            svg_text(1171, 78, "contributions / 365 days", 16, theme["muted"], mono=True, anchor="end"),
        ]
    for weekday, label in [(1, "Mon"), (3, "Wed"), (5, "Fri")]:
        body.append(svg_text(grid_x - 15, grid_y + weekday * step + cell * 0.78, label, 18 if mobile else 13, theme["muted"], anchor="end"))
    last_month = None
    for column in range(weeks):
        week_start = start_week + timedelta(weeks=column)
        month_date = max(week_start, first)
        if not mobile and month_date.month != last_month and column < weeks - 1:
            body.append(svg_text(grid_x + column * step, grid_y - 13, month_date.strftime("%b"), 13, theme["muted"]))
            last_month = month_date.month
        for weekday in range(7):
            current = week_start + timedelta(days=weekday)
            item = days.get(current)
            if item is None or current > end:
                continue
            level = max(0, min(4, int(item["level"])))
            x, y = grid_x + column * step, grid_y + weekday * step
            body.append(
                f'<g><title>{current.isoformat()}: {count(item["count"])} contributions</title>'
                + rect(x, y, cell, cell, theme["levels"][level], 4 if mobile else 3)
                + "</g>"
            )
    today_x = grid_x + (weeks - 1) * step
    today_y = grid_y + ((end.weekday() + 1) % 7) * step
    body.append(rect(today_x - 2, today_y - 2, cell + 4, cell + 4, "none", 5, theme["accent"]))
    body.append(
        f'<g class="motion" opacity=".6"><circle cx="{grid_x:g}" cy="{grid_y - 7:g}" r="2" fill="{theme["accent"]}">'
        f'<animate attributeName="cx" values="{grid_x:g};{grid_x + (weeks - 1) * step:g};{grid_x:g}" dur="36s" repeatCount="indefinite"/>'
        "</circle></g>"
    )
    legend_y = 449 if mobile else 306
    legend_x = 457 if mobile else 964
    body.append(svg_text(legend_x - 17, legend_y + 10, "Less", 17 if mobile else 14, theme["muted"], anchor="end"))
    for level in range(5):
        body.append(rect(legend_x + level * 21, legend_y - 3, 15, 15, theme["levels"][level], 3))
    body.append(svg_text(legend_x + 114, legend_y + 10, "More", 17 if mobile else 14, theme["muted"]))
    if not mobile:
        body.append(svg_text(28, 316, "One square. One day. Your actual GitHub history.", 15, theme["muted"]))
    description = (
        f'{count(total)} contributions on the GitHub profile calendar from {first} to {end}. '
        + ("This mobile view displays the most recent 13 weeks. " if mobile else "")
        + "An outlined square marks the current day. GitHub may include anonymized private contributions enabled by the account owner."
    )
    return document(width, height, "GitHub contribution calendar", description, body, theme)


def picture(name: str, alt: str, width: str = "100%") -> str:
    path = "./assets/generated/"
    return (
        "<picture>\n"
        f'  <source media="(max-width: 600px) and (prefers-color-scheme: dark)" srcset="{path}{name}-mobile-dark.svg">\n'
        f'  <source media="(max-width: 600px)" srcset="{path}{name}-mobile-light.svg">\n'
        f'  <source media="(prefers-color-scheme: dark)" srcset="{path}{name}-dark.svg">\n'
        f'  <img src="{path}{name}-light.svg" alt="{esc(alt)}" width="{width}">\n'
        "</picture>"
    )


def readme(data: dict) -> str:
    profile = data["profile"]
    stats = data["stats"]
    login = profile["login"]
    name = profile.get("name") or login
    summary = (
        f'{name}, @{login}. {profile.get("bio") or INTRODUCTION} '
        f'{stats["original_repos"]} original public repositories, {stats["total_stars"]} project stars, '
        f'{profile["followers"]} followers.'
    )
    body = [
        picture("hero", summary),
        "",
        '<p align="center">',
        '  <a href="https://t.me/WhyFaust_tv">',
        picture("telegram", "Telegram · @WhyFaust_tv", "49%"),
        "  </a>",
        f'  <a href="https://github.com/{esc(login)}?tab=repositories">',
        picture("repositories", "Browse all GitHub repositories", "49%"),
        "  </a>",
        "</p>",
        "",
        "## Currently building",
        "",
        "Latest original public projects, automatically selected from GitHub.",
        "",
    ]
    projects = data.get("projects", [])[:4]
    for index, project in enumerate(projects, 1):
        alt = f'{project["name"]}. {project.get("description") or "Explore this project on GitHub."} {project["stars"]} stars.'
        body.extend([
            f'<a href="{esc(project["url"])}">',
            picture(f"project-{index}", alt),
            "</a>",
            "",
        ])
    if not projects:
        body.extend([f'[Explore @{login} on GitHub](https://github.com/{login}?tab=repositories).', ""])
    body.extend([
        "## A year on GitHub",
        "",
        picture("activity", f'{data["contributions"]["total"]} GitHub contributions over the last 365 days. Mobile shows the most recent 13 weeks.'),
        "",
        '<p align="center">',
        '  <a href="https://t.me/WhyFaust_tv">AI, Python, development &amp; games — @WhyFaust_tv</a>',
        "</p>",
        "",
    ])
    stamp = datetime.fromisoformat(data["updated_at"].replace("Z", "+00:00")).astimezone(timezone.utc)
    body.extend([
        '<p align="center"><sub>',
        f'  From GitHub, automatically. Snapshot changed {stamp:%d %b %Y, %H:%M UTC}.<br>',
        f'  <a href="https://github.com/{esc(login)}/{esc(login)}/actions/workflows/update-profile.yml">Refreshes on a 5-minute schedule</a> · GitHub may delay updates.',
        "</sub></p>",
        "",
    ])
    return "\n".join(body)


def render(data: dict, root: Path) -> None:
    root = Path(root)
    destination = root / "assets" / "generated"
    image = image_uri(root)
    output = {}
    for name, theme in THEMES.items():
        for mobile in (False, True):
            suffix = f"-mobile-{name}" if mobile else f"-{name}"
            output[f"hero{suffix}.svg"] = hero(data, image, theme, mobile)
            output[f"telegram{suffix}.svg"] = button("Telegram", "@WhyFaust_tv", theme, True, mobile)
            output[f"repositories{suffix}.svg"] = button("Projects", "Browse the source on GitHub", theme, False, mobile)
            output[f"activity{suffix}.svg"] = activity(data, theme, mobile)
            for index, project in enumerate(data.get("projects", [])[:4], 1):
                output[f"project-{index}{suffix}.svg"] = project_card(project, index, theme, mobile)
    contents = readme(data)
    destination.mkdir(parents=True, exist_ok=True)
    for filename, content in output.items():
        target = destination / filename
        if not target.is_file() or target.read_text(encoding="utf-8") != content:
            target.write_text(content, encoding="utf-8")
    readme_path = root / "README.md"
    if not readme_path.is_file() or readme_path.read_text(encoding="utf-8") != contents:
        readme_path.write_text(contents, encoding="utf-8")


if __name__ == "__main__":
    project_root = Path(__file__).resolve().parents[1]
    render(json.loads((project_root / "data" / "profile.json").read_text(encoding="utf-8")), project_root)
