use chrono::{Duration as CalendarDuration, NaiveDate, SecondsFormat, Utc};
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::io::{self, Read};
use std::time::Duration;

type CollectResult<T> = Result<T, Box<dyn Error>>;

fn data_error(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn text<'a>(value: Option<&'a Value>, field: &str, nullable: bool) -> CollectResult<&'a str> {
    match value {
        Some(Value::String(value)) => Ok(value),
        Some(Value::Null) if nullable => Ok(""),
        _ => Err(data_error(format!("Expected text for {field}"))),
    }
}

fn count(value: Option<&Value>, field: &str) -> CollectResult<u64> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| data_error(format!("Expected a nonnegative integer for {field}")))
}

fn required_value<'a>(
    payload: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> CollectResult<&'a Value> {
    payload
        .get(key)
        .ok_or_else(|| data_error(format!("GitHub {context} field {key} is missing")))
}

fn selector(value: &str) -> CollectResult<Selector> {
    Selector::parse(value)
        .map_err(|_| data_error(format!("Invalid GitHub document selector: {value}")))
}

fn request_text(agent: &ureq::Agent, url: &str, api: bool) -> CollectResult<String> {
    let mut request = agent
        .get(url)
        .set("User-Agent", "WhyFaust-profile")
        .set("Accept-Language", "en-US");
    if api {
        if !url.starts_with("https://api.github.com/") {
            return Err(data_error("GitHub API request has an invalid origin"));
        }
        request = request
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                request = request.set("Authorization", &format!("Bearer {token}"));
            }
        }
    } else {
        request = request.set("Accept", "text/html");
    }
    let response = request.call().map_err(|error| match error {
        ureq::Error::Status(status, _) => {
            data_error(format!("GitHub request failed with HTTP {status}"))
        }
        ureq::Error::Transport(_) => {
            data_error("GitHub response could not be retrieved or decoded")
        }
    })?;
    if !(200..300).contains(&response.status()) {
        return Err(data_error(format!(
            "GitHub request failed with HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|_| data_error("GitHub response could not be retrieved or decoded"))?;
    String::from_utf8(bytes)
        .map_err(|_| data_error("GitHub response could not be retrieved or decoded"))
}

fn api_json(agent: &ureq::Agent, path: &str) -> CollectResult<Value> {
    let document = request_text(agent, &format!("https://api.github.com{path}"), true)?;
    serde_json::from_str(&document).map_err(|_| data_error("GitHub API returned invalid JSON"))
}

fn get_repositories(agent: &ureq::Agent, login: &str) -> CollectResult<Vec<Value>> {
    let mut repositories = Vec::new();
    let mut names = HashSet::new();
    let mut page = 1_u64;
    loop {
        let payload = api_json(
            agent,
            &format!(
                "/users/{login}/repos?type=owner&sort=full_name&direction=asc&per_page=100&page={page}"
            ),
        )?;
        let entries = payload
            .as_array()
            .ok_or_else(|| data_error("GitHub repositories response is not a list"))?;
        for entry in entries {
            let repository = entry
                .as_object()
                .ok_or_else(|| data_error("GitHub repository is not an object"))?;
            let owner = repository
                .get("owner")
                .and_then(Value::as_object)
                .ok_or_else(|| data_error("GitHub repository owner is missing"))?;
            let private = repository
                .get("private")
                .and_then(Value::as_bool)
                .ok_or_else(|| data_error("GitHub repository visibility is missing"))?;
            let visibility = repository.get("visibility").and_then(Value::as_str);
            if !matches!(visibility, Some("public" | "private" | "internal")) {
                return Err(data_error("GitHub repository visibility is missing"));
            }
            if private || visibility != Some("public") {
                continue;
            }
            if !text(owner.get("login"), "repository owner", false)?
                .eq_ignore_ascii_case(login)
            {
                continue;
            }
            let name = text(repository.get("name"), "repository name", false)?;
            if !names.insert(name.to_owned()) {
                return Err(data_error(
                    "GitHub repository pagination returned duplicate entries",
                ));
            }
            repositories.push(entry.clone());
        }
        if entries.len() < 100 {
            return Ok(repositories);
        }
        page = page
            .checked_add(1)
            .ok_or_else(|| data_error("GitHub repository pagination exceeded its limit"))?;
    }
}

fn parse_pins(document: &str, login: &str) -> CollectResult<Vec<String>> {
    let document = Html::parse_document(document);
    let sections = selector(".js-pinned-items-reorder-container")?;
    let headings = selector("h2")?;
    let links = selector(".pinned-item-list-item a[href]")?;
    let repository_link = Regex::new(r"^/[A-Za-z0-9-]+/[A-Za-z0-9_.-]+$")?;
    let mut found_section = false;
    let mut pinned = false;
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for section in document.select(&sections) {
        for heading in section.select(&headings) {
            let heading = heading.text().collect::<Vec<_>>().join(" ");
            let heading = heading.split_whitespace().collect::<Vec<_>>().join(" ");
            found_section |=
                heading.starts_with("Pinned") || heading.starts_with("Popular repositories");
            pinned |= heading.starts_with("Pinned");
        }
        for link in section.select(&links) {
            let href = link.value().attr("href").unwrap_or_default();
            if repository_link.is_match(href) && seen.insert(href.to_owned()) {
                if let Some((owner, name)) = href[1..].split_once('/') {
                    if owner.eq_ignore_ascii_case(login) {
                        names.push(name.to_owned());
                    }
                }
            }
        }
    }
    if !found_section {
        return Err(data_error("GitHub profile repository section is missing"));
    }
    Ok(if pinned { names } else { Vec::new() })
}

fn parse_contributions(document: &str, start: NaiveDate, end: NaiveDate) -> CollectResult<Value> {
    let document = Html::parse_document(document);
    let cells = selector(".ContributionCalendar-day[data-date]")?;
    let tooltip_elements = selector("tool-tip[for]")?;
    let count_pattern = Regex::new(r"^(No|[0-9][0-9,]*) contributions?\b")?;
    let mut tooltips = HashMap::new();
    for tooltip in document.select(&tooltip_elements) {
        if let Some(target) = tooltip.value().attr("for") {
            let label = tooltip.text().collect::<Vec<_>>().join(" ");
            tooltips.insert(target.to_owned(), label.trim().to_owned());
        }
    }
    let mut identifiers = HashSet::new();
    let mut days = BTreeMap::new();
    let mut total = 0_u64;
    for cell in document.select(&cells) {
        let attributes = cell.value();
        let identifier = attributes
            .attr("id")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| data_error("GitHub calendar cell is incomplete"))?;
        let day = attributes
            .attr("data-date")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| data_error("GitHub calendar cell is incomplete"))?;
        let raw_level = attributes
            .attr("data-level")
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or_else(|| data_error("GitHub calendar cell is incomplete"))?;
        let level = raw_level
            .parse::<u64>()
            .map_err(|_| data_error("GitHub calendar cell is incomplete"))?;
        if !identifiers.insert(identifier.to_owned()) {
            return Err(data_error("GitHub calendar contains duplicate cells"));
        }
        let current_date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map_err(|_| data_error("GitHub calendar contains an invalid date"))?;
        if current_date < start || current_date > end {
            continue;
        }
        let label = tooltips.get(identifier).map(String::as_str).unwrap_or("");
        let captures = count_pattern.captures(label).ok_or_else(|| {
            data_error(format!("GitHub calendar count missing for {current_date}"))
        })?;
        let amount = if &captures[1] == "No" {
            0
        } else {
            captures[1].replace(',', "").parse::<u64>().map_err(|_| {
                data_error(format!("GitHub calendar count invalid for {current_date}"))
            })?
        };
        if level > 4 || (amount == 0) != (level == 0) {
            return Err(data_error(format!(
                "GitHub calendar intensity mismatch for {current_date}"
            )));
        }
        let value = json!({
            "date": current_date.to_string(),
            "count": amount,
            "level": level,
        });
        if days.insert(current_date, value).is_some() {
            return Err(data_error("GitHub calendar contains duplicate dates"));
        }
        total = total
            .checked_add(amount)
            .ok_or_else(|| data_error("GitHub calendar count exceeded its limit"))?;
    }
    let expected_days = (end - start).num_days() + 1;
    if days.len() as i64 != expected_days
        || (0..expected_days).any(|offset| {
            !days.contains_key(&(start + CalendarDuration::days(offset)))
        })
    {
        return Err(data_error(
            "GitHub calendar is incomplete; previous published data retained",
        ));
    }
    Ok(json!({
        "total": total,
        "start": start.to_string(),
        "end": end.to_string(),
        "days": days.into_values().collect::<Vec<_>>(),
    }))
}

fn language_color(language: &str) -> &'static str {
    match language {
        "Python" => "#e3b341",
        "JavaScript" => "#f1e05a",
        "TypeScript" => "#3178c6",
        "C#" => "#a178df",
        "SourcePawn" => "#ed9569",
        "HTML" => "#e34c26",
        "CSS" => "#9b7edb",
        "Shell" => "#89b97e",
        "Rust" => "#dea584",
        "Go" => "#00add8",
        _ => "#a8afb8",
    }
}

struct Repository<'a> {
    name: &'a str,
    description: &'a str,
    language: &'a str,
    pushed_at: &'a str,
    stars: u64,
    forks: u64,
    archived: bool,
    disabled: bool,
}

fn normalize_repositories(
    repositories: &[Value],
    login: &str,
    pins: &[String],
) -> CollectResult<(Value, Value, Value)> {
    let mut originals = Vec::new();
    let mut total_stars = 0_u64;
    let mut total_forks = 0_u64;
    let mut language_counts = HashMap::new();
    for value in repositories {
        let repository = value
            .as_object()
            .ok_or_else(|| data_error("GitHub repository is not an object"))?;
        let status = |field: &str| {
            repository
                .get(field)
                .and_then(Value::as_bool)
                .ok_or_else(|| data_error("GitHub repository status is missing"))
        };
        let fork = status("fork")?;
        let archived = status("archived")?;
        let disabled = status("disabled")?;
        let name = text(repository.get("name"), "repository name", false)?;
        if fork
            || name.eq_ignore_ascii_case(login)
            || name.eq_ignore_ascii_case("github-profile-summary-cards")
        {
            continue;
        }
        let description = text(
            Some(required_value(repository, "description", "repository")?),
            "repository description",
            true,
        )?;
        let language = text(
            Some(required_value(repository, "language", "repository")?),
            "repository language",
            true,
        )?;
        let pushed_at = text(
            Some(required_value(repository, "pushed_at", "repository")?),
            "repository push date",
            true,
        )?;
        let stars = count(repository.get("stargazers_count"), "repository stars")?;
        let forks = count(repository.get("forks_count"), "repository forks")?;
        total_stars = total_stars
            .checked_add(stars)
            .ok_or_else(|| data_error("GitHub repository star count exceeded its limit"))?;
        total_forks = total_forks
            .checked_add(forks)
            .ok_or_else(|| data_error("GitHub repository fork count exceeded its limit"))?;
        if !language.is_empty() {
            *language_counts.entry(language).or_insert(0_usize) += 1;
        }
        originals.push(Repository {
            name,
            description,
            language,
            pushed_at,
            stars,
            forks,
            archived,
            disabled,
        });
    }
    let stats = json!({
        "original_repos": originals.len(),
        "total_stars": total_stars,
        "total_forks": total_forks,
    });
    let mut language_counts = language_counts.into_iter().collect::<Vec<_>>();
    language_counts.sort_by(|(left_name, left_count), (right_name, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_name.cmp(right_name))
    });
    let languages = language_counts
        .into_iter()
        .map(|(name, amount)| {
            json!({"name": name, "repo_count": amount, "color": language_color(name)})
        })
        .collect::<Vec<_>>();
    let mut candidates = originals
        .iter()
        .filter(|repository| {
            !repository.archived && !repository.disabled && !repository.pushed_at.is_empty()
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .pushed_at
            .cmp(left.pushed_at)
            .then_with(|| right.name.cmp(left.name))
    });
    let pinned_order = pins
        .iter()
        .enumerate()
        .map(|(index, name)| (name.to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    candidates.sort_by_key(|repository| {
        pinned_order
            .get(&repository.name.to_ascii_lowercase())
            .copied()
            .unwrap_or(pins.len())
    });
    let projects = candidates
        .into_iter()
        .take(4)
        .map(|repository| {
            json!({
                "name": repository.name,
                "description": repository.description,
                "url": format!("https://github.com/{login}/{}", repository.name),
                "stars": repository.stars,
                "forks": repository.forks,
                "language": repository.language,
                "pushed_at": repository.pushed_at,
            })
        })
        .collect::<Vec<_>>();
    Ok((stats, json!(languages), json!(projects)))
}

pub fn collect_profile(username: &str) -> Result<Value, Box<dyn Error>> {
    let valid_username = Regex::new(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$")?;
    if !valid_username.is_match(username) {
        return Err(data_error("Invalid GitHub username"));
    }
    let agent = ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(Duration::from_secs(30))
        .redirects(0)
        .build();
    let payload = api_json(&agent, &format!("/users/{username}"))?;
    let payload = payload
        .as_object()
        .filter(|payload| payload.get("type").and_then(Value::as_str) == Some("User"))
        .ok_or_else(|| data_error("Expected a GitHub user profile"))?;
    let login = text(payload.get("login"), "profile login", false)?;
    if !login.eq_ignore_ascii_case(username) {
        return Err(data_error("GitHub returned a different account"));
    }
    let name = text(
        Some(required_value(payload, "name", "profile")?),
        "profile name",
        true,
    )?;
    let public_repos = count(payload.get("public_repos"), "public repositories")?;
    let profile_url = format!("https://github.com/{login}");
    let profile = json!({
        "login": login,
        "name": if name.is_empty() { login } else { name },
        "bio": text(Some(required_value(payload, "bio", "profile")?), "profile bio", true)?,
        "location": text(Some(required_value(payload, "location", "profile")?), "profile location", true)?,
        "url": profile_url,
        "created_at": text(payload.get("created_at"), "profile creation date", false)?,
        "followers": count(payload.get("followers"), "followers")?,
        "following": count(payload.get("following"), "following")?,
        "public_repos": public_repos,
    });
    let repositories = get_repositories(&agent, login)?;
    if repositories.len() as u64 != public_repos {
        return Err(data_error(
            "GitHub public repository listing is incomplete or changed during refresh",
        ));
    }
    let pins = parse_pins(&request_text(&agent, &profile_url, false)?, login)?;
    let (stats, languages, projects) = normalize_repositories(&repositories, login, &pins)?;
    let end = Utc::now().date_naive();
    let start = end - CalendarDuration::days(364);
    let calendar = request_text(
        &agent,
        &format!("https://github.com/users/{login}/contributions"),
        false,
    )?;
    let contributions = parse_contributions(&calendar, start, end)?;
    Ok(json!({
        "profile": profile,
        "stats": stats,
        "languages": languages,
        "projects": projects,
        "contributions": contributions,
    }))
}

pub fn snapshot_with_timestamp(mut data: Value, previous: Option<&Value>) -> Value {
    let timestamp = previous
        .filter(|previous| {
            let Some(previous) = previous.as_object() else {
                return false;
            };
            let mut previous_payload = previous.clone();
            previous_payload.remove("updated_at");
            data == Value::Object(previous_payload)
        })
        .and_then(|previous| previous.get("updated_at"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));
    data["updated_at"] = Value::String(timestamp);
    data
}
