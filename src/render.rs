use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

type RenderResult<T> = Result<T, Box<dyn Error>>;

const MONO: &str = "ui-monospace, SFMono-Regular, Consolas, Liberation Mono, monospace";
const SANS: &str = "-apple-system, BlinkMacSystemFont, Segoe UI, Arial, sans-serif";
const INTRODUCTION: &str = "QA automation engineer. Python, integrations and developer tools.";

struct Theme {
    background: &'static str,
    surface: &'static str,
    border: &'static str,
    text: &'static str,
    muted: &'static str,
    accent: &'static str,
    accent_text: &'static str,
    soft: &'static str,
    levels: [&'static str; 5],
}

const THEMES: [(&str, Theme); 2] = [
    (
        "dark",
        Theme {
            background: "#171513",
            surface: "#24201c",
            border: "#43382f",
            text: "#fff5e8",
            muted: "#c5b2a3",
            accent: "#f0a17b",
            accent_text: "#281810",
            soft: "#a6bd9b",
            levels: ["#302923", "#674234", "#a45b3e", "#db9164", "#facb95"],
        },
    ),
    (
        "light",
        Theme {
            background: "#fffaf2",
            surface: "#f4eadd",
            border: "#dfcdbb",
            text: "#34271f",
            muted: "#796252",
            accent: "#a64a2c",
            accent_text: "#fffaf2",
            soft: "#58774c",
            levels: ["#eee2d3", "#edc6ae", "#d9936c", "#bc6840", "#874126"],
        },
    ),
];

struct Language<'a> {
    name: &'a str,
    repo_count: u64,
}

struct Project<'a> {
    name: &'a str,
    description: &'a str,
    url: &'a str,
    stars: u64,
    forks: u64,
    language: &'a str,
    pushed: NaiveDate,
}

struct Day {
    count: u64,
    level: usize,
}

struct Calendar {
    first: NaiveDate,
    end: NaiveDate,
    total: u64,
    days: BTreeMap<NaiveDate, Day>,
}

struct Snapshot<'a> {
    login: &'a str,
    name: &'a str,
    biography: &'a str,
    location: Option<&'a str>,
    created: NaiveDate,
    followers: u64,
    original_repos: u64,
    total_stars: u64,
    languages: Vec<Language<'a>>,
    projects: Vec<Project<'a>>,
    calendar: Calendar,
    updated_at: DateTime<Utc>,
}

fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn string<'a>(data: &'a Value, path: &str) -> RenderResult<&'a str> {
    data.pointer(path)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{path} must be a string")))
}

fn optional_string<'a>(data: &'a Value, path: &str) -> RenderResult<Option<&'a str>> {
    match data.pointer(path) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok((!value.is_empty()).then_some(value.as_str())),
        Some(_) => Err(invalid(format!("{path} must be a string or null"))),
    }
}

fn number(data: &Value, path: &str) -> RenderResult<u64> {
    data.pointer(path)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("{path} must be a nonnegative integer")))
}

fn array<'a>(data: &'a Value, path: &str, optional: bool) -> RenderResult<&'a [Value]> {
    match data.pointer(path) {
        Some(Value::Array(values)) => Ok(values),
        None if optional => Ok(&[]),
        _ => Err(invalid(format!("{path} must be an array"))),
    }
}

fn day(value: &str, path: &str) -> RenderResult<NaiveDate> {
    let prefix = value
        .get(..10)
        .ok_or_else(|| invalid(format!("{path} must start with a date in YYYY-MM-DD format")))?;
    NaiveDate::parse_from_str(prefix, "%Y-%m-%d")
        .map_err(|error| invalid(format!("{path} has an invalid date: {error}")))
}

impl<'a> Snapshot<'a> {
    fn from_json(data: &'a Value) -> RenderResult<Self> {
        let login = string(data, "/profile/login")?;
        if clean(login).trim().is_empty() {
            return Err(invalid("/profile/login must not be empty"));
        }
        let mut languages = Vec::new();
        for (index, item) in array(data, "/languages", true)?.iter().enumerate() {
            languages.push(Language {
                name: string(item, "/name")
                    .map_err(|error| invalid(format!("/languages/{index}: {error}")))?,
                repo_count: number(item, "/repo_count")
                    .map_err(|error| invalid(format!("/languages/{index}: {error}")))?,
            });
        }
        let mut projects = Vec::new();
        for (index, item) in array(data, "/projects", true)?.iter().take(4).enumerate() {
            let project = (|| -> RenderResult<Project<'a>> {
                let url = string(item, "/url")?;
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err(invalid("/url must be an HTTP or HTTPS URL"));
                }
                Ok(Project {
                    name: string(item, "/name")?,
                    description: optional_string(item, "/description")?.unwrap_or(""),
                    url,
                    stars: number(item, "/stars")?,
                    forks: number(item, "/forks")?,
                    language: optional_string(item, "/language")?.unwrap_or("Mixed"),
                    pushed: day(string(item, "/pushed_at")?, "/pushed_at")?,
                })
            })()
            .map_err(|error| invalid(format!("/projects/{index}: {error}")))?;
            projects.push(project);
        }
        let first = day(string(data, "/contributions/start")?, "/contributions/start")?;
        let end = day(string(data, "/contributions/end")?, "/contributions/end")?;
        if end.signed_duration_since(first).num_days() != 364 {
            return Err(invalid("/contributions must cover exactly 365 inclusive days"));
        }
        let mut days = BTreeMap::new();
        let mut sum = 0_u64;
        for (index, item) in array(data, "/contributions/days", false)?.iter().enumerate() {
            let parsed = (|| -> RenderResult<(NaiveDate, Day)> {
                let date = day(string(item, "/date")?, "/date")?;
                let count = number(item, "/count")?;
                let level = number(item, "/level")?;
                if !(first..=end).contains(&date) || level > 4 {
                    return Err(invalid("date must be inside the calendar and level must be 0–4"));
                }
                Ok((date, Day { count, level: level as usize }))
            })()
            .map_err(|error| invalid(format!("/contributions/days/{index}: {error}")))?;
            sum = sum
                .checked_add(parsed.1.count)
                .ok_or_else(|| invalid("contribution count exceeds the supported integer range"))?;
            if days.insert(parsed.0, parsed.1).is_some() {
                return Err(invalid(format!("duplicate contribution date: {}", parsed.0)));
            }
        }
        let total = number(data, "/contributions/total")?;
        if days.len() != 365 || total != sum {
            return Err(invalid("/contributions needs every date once and a total equal to the day sum"));
        }
        let updated_at = DateTime::parse_from_rfc3339(string(data, "/updated_at")?)
            .map_err(|error| invalid(format!("/updated_at is invalid: {error}")))?
            .with_timezone(&Utc);
        Ok(Self {
            login,
            name: optional_string(data, "/profile/name")?.unwrap_or(login),
            biography: optional_string(data, "/profile/bio")?.unwrap_or(INTRODUCTION),
            location: optional_string(data, "/profile/location")?,
            created: day(string(data, "/profile/created_at")?, "/profile/created_at")?,
            followers: number(data, "/profile/followers")?,
            original_repos: number(data, "/stats/original_repos")?,
            total_stars: number(data, "/stats/total_stars")?,
            languages,
            projects,
            calendar: Calendar { first, end, total, days },
            updated_at,
        })
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|&character| {
            matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
        })
        .collect()
}

fn esc(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in clean(value).chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(character),
        }
    }
    result
}

fn normalized(value: &str) -> String {
    clean(value).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn short(value: &str, width: usize) -> String {
    let value = normalized(value);
    if width == 0 {
        return String::new();
    }
    if value.chars().count() <= width {
        return value;
    }
    let prefix: String = value.chars().take(width - 1).collect();
    format!("{}…", prefix.trim_end())
}

fn lines(value: &str, width: usize, limit: usize) -> Vec<String> {
    if width == 0 || limit == 0 {
        return Vec::new();
    }
    let value = normalized(value);
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut length = 0;
    for word in value.split_whitespace() {
        let characters: Vec<char> = word.chars().collect();
        if length > 0 && length + 1 + characters.len() > width {
            wrapped.push(std::mem::take(&mut current));
            length = 0;
        }
        if characters.len() > width {
            for chunk in characters.chunks(width) {
                if chunk.len() == width {
                    wrapped.push(chunk.iter().collect());
                } else {
                    current = chunk.iter().collect();
                    length = chunk.len();
                }
            }
        } else {
            if length > 0 {
                current.push(' ');
                length += 1;
            }
            current.push_str(word);
            length += characters.len();
        }
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    if wrapped.len() > limit {
        wrapped.truncate(limit);
        if let Some(last) = wrapped.last_mut() {
            let prefix: String = last.chars().take(width - 1).collect();
            *last = format!("{}…", prefix.trim_end().trim_end_matches('…'));
        }
    }
    wrapped
}

fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn svg_text(
    x: f64,
    y: f64,
    value: &str,
    size: u32,
    color: &str,
    weight: u32,
    mono: bool,
    anchor: &str,
) -> String {
    let font = if mono { MONO } else { SANS };
    format!(
        "<text x=\"{x}\" y=\"{y}\" fill=\"{color}\" font-size=\"{size}\" font-family=\"{font}\" font-weight=\"{weight}\" text-anchor=\"{anchor}\">{}</text>",
        esc(value)
    )
}

fn rect(x: f64, y: f64, width: f64, height: f64, fill: &str, radius: f64, stroke: Option<&str>) -> String {
    let stroke = stroke.map(|color| format!(" stroke=\"{color}\"")).unwrap_or_default();
    format!("<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{radius}\" fill=\"{fill}\"{stroke}/>")
}

fn document(width: u32, height: u32, title: &str, description: &str, body: Vec<String>, theme: &Theme) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-labelledby=\"title description\">\n<title id=\"title\">{}</title><desc id=\"description\">{}</desc>\n<style>@media (prefers-reduced-motion: reduce){{.motion{{display:none}}}}</style>\n{}\n{}\n</svg>\n",
        esc(title),
        esc(description),
        rect(1.0, 1.0, (width - 2) as f64, (height - 2) as f64, theme.background, 22.0, Some(theme.border)),
        body.join("\n")
    )
}

fn image_uri(root: &Path) -> RenderResult<String> {
    let jpeg = root.join("assets/mascot.jpg");
    let (path, mime) = if jpeg.is_file() {
        (jpeg, "image/jpeg")
    } else {
        (root.join("assets/mascot.png"), "image/png")
    };
    let bytes = fs::read(&path)?;
    if bytes.is_empty() {
        return Err(invalid(format!("portrait is empty: {}", path.display())));
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn hero(snapshot: &Snapshot<'_>, image: &str, theme: &Theme) -> String {
    let year_label = match snapshot.location {
        Some(location) => format!("{}  ·  Since {}", short(location, 22), snapshot.created.year()),
        None => format!("On GitHub since {}", snapshot.created.format("%B %Y")),
    };
    let mut body = vec![
        format!("<path d=\"M1 54H1199\" stroke=\"{}\"/>", theme.border),
        format!("<circle cx=\"27\" cy=\"28\" r=\"5\" fill=\"{}\"/>", theme.accent),
        format!("<circle cx=\"46\" cy=\"28\" r=\"5\" fill=\"{}\" opacity=\".65\"/>", theme.muted),
        format!("<circle cx=\"65\" cy=\"28\" r=\"5\" fill=\"{}\"/>", theme.soft),
        svg_text(90.0, 34.0, &format!("{}@github: ~", snapshot.login.to_lowercase()), 16, theme.muted, 400, true, "start"),
        svg_text(1174.0, 34.0, "PERSONAL WORKSPACE", 14, theme.muted, 400, true, "end"),
        "<defs><clipPath id=\"portrait\"><rect x=\"28\" y=\"80\" width=\"430\" height=\"430\" rx=\"18\"/></clipPath><linearGradient id=\"portrait-sheen\"><stop offset=\"0\" stop-color=\"#fff5e8\" stop-opacity=\"0\"/><stop offset=\".5\" stop-color=\"#fff5e8\" stop-opacity=\".28\"/><stop offset=\"1\" stop-color=\"#fff5e8\" stop-opacity=\"0\"/></linearGradient></defs>".into(),
        format!("<image x=\"28\" y=\"80\" width=\"430\" height=\"430\" href=\"{image}\" clip-path=\"url(#portrait)\"/>"),
    ];
    let mut portrait_motion = String::from(
        "<g class=\"motion\" clip-path=\"url(#portrait)\"><g transform=\"translate(28 80) scale(0.671875)\"><rect x=\"-240\" y=\"0\" width=\"210\" height=\"640\" fill=\"url(#portrait-sheen)\" opacity=\"0\"><animate attributeName=\"x\" values=\"-240;720\" dur=\"8s\" begin=\"-2s\" repeatCount=\"indefinite\"/><animate attributeName=\"opacity\" values=\"0;1;1;0\" keyTimes=\"0;.15;.8;1\" dur=\"8s\" begin=\"-2s\" repeatCount=\"indefinite\"/></rect>",
    );
    for (index, path) in [
        "M181 424 C169 410 194 402 182 387",
        "M205 426 C218 412 193 400 207 379",
        "M229 429 C216 416 239 405 227 390",
    ].iter().enumerate() {
        let begin = -(index as f64 * 1.6 + 0.8);
        portrait_motion.push_str(&format!(
            "<path d=\"{path}\" fill=\"none\" stroke=\"#fff5e8\" stroke-width=\"4\" stroke-linecap=\"round\" opacity=\"0\"><animate attributeName=\"opacity\" values=\"0;.75;.45;0\" keyTimes=\"0;.22;.64;1\" dur=\"4.8s\" begin=\"{begin}s\" repeatCount=\"indefinite\" calcMode=\"spline\" keySplines=\".4 0 .6 1;.4 0 .6 1;.4 0 .6 1\"/><animateTransform attributeName=\"transform\" type=\"translate\" values=\"0 12;4 -48\" dur=\"4.8s\" begin=\"{begin}s\" repeatCount=\"indefinite\"/></path>"
        ));
    }
    portrait_motion.push_str("</g></g>");
    body.push(portrait_motion);
    body.extend([
        svg_text(506.0, 109.0, "$ whoami", 19, theme.accent, 400, true, "start"),
        svg_text(502.0, 181.0, &short(snapshot.name, 16), 68, theme.text, 750, false, "start"),
        svg_text(506.0, 220.0, &format!("@{}", snapshot.login), 22, theme.accent, 400, true, "start"),
        svg_text(41.0, 554.0, "CODE  /  COFFEE  /  CURIOSITY", 17, theme.muted, 400, true, "start"),
    ]);
    for (index, line) in lines(snapshot.biography, 52, 3).iter().enumerate() {
        body.push(svg_text(506.0, 262.0 + index as f64 * 29.0, line, 22, theme.text, 400, false, "start"));
    }
    body.push(svg_text(506.0, 355.0, &year_label, 18, theme.muted, 400, false, "start"));
    body.push(format!(
        "<rect class=\"motion\" x=\"609\" y=\"111\" width=\"13\" height=\"3\" rx=\"1\" fill=\"{}\"><animate attributeName=\"opacity\" values=\".15;1;1;.15\" keyTimes=\"0;.3;.7;1\" dur=\"2.8s\" repeatCount=\"indefinite\"/></rect>",
        theme.accent
    ));
    for (index, (number, label)) in [
        (snapshot.original_repos, "original repos"),
        (snapshot.total_stars, "project stars"),
        (snapshot.followers, "followers"),
    ].iter().enumerate() {
        let x = 506.0 + index as f64 * 220.0;
        body.push(rect(x, 384.0, 206.0, 102.0, theme.surface, 12.0, None));
        body.push(svg_text(x + 18.0, 432.0, &count(*number), 36, theme.text, 650, false, "start"));
        body.push(svg_text(x + 18.0, 465.0, label, 16, theme.muted, 400, false, "start"));
    }
    let mut language_text = snapshot.languages.iter().take(3)
        .map(|item| format!("{} {}", item.name, item.repo_count))
        .collect::<Vec<_>>().join("  ·  ");
    if language_text.is_empty() {
        language_text = "New projects are on the way.".into();
    }
    body.extend([
        svg_text(506.0, 526.0, "PRIMARY LANGUAGES / REPO COUNT", 13, theme.muted, 400, true, "start"),
        svg_text(506.0, 557.0, &short(&language_text, 49), 18, theme.accent, 400, true, "start"),
    ]);
    let description = format!(
        "{}, @{}. {} {} original public repositories excluding this profile; {} stars on those repositories; {} followers. Languages show primary-language repository counts.",
        snapshot.name, snapshot.login, snapshot.biography, snapshot.original_repos, snapshot.total_stars, snapshot.followers
    );
    document(1200, 596, &format!("{} | GitHub portfolio", snapshot.name), &description, body, theme)
}

fn button(label: &str, detail: &str, theme: &Theme, primary: bool) -> String {
    let foreground = if primary { theme.accent_text } else { theme.text };
    let background = if primary { theme.accent } else { theme.surface };
    let body = vec![
        rect(1.0, 1.0, 578.0, 90.0, background, 16.0, None),
        svg_text(28.0, 41.0, label, 25, foreground, 600, false, "start"),
        svg_text(28.0, 69.0, detail, 17, foreground, 400, false, "start"),
        svg_text(550.0, 58.0, "↗", 32, foreground, 400, false, "end"),
    ];
    document(580, 92, label, detail, body, theme)
}

fn project_card(project: &Project<'_>, index: usize, theme: &Theme) -> String {
    let description = if project.description.is_empty() {
        "Explore the source, documentation and latest changes on GitHub."
    } else {
        project.description
    };
    let mut body = vec![
        svg_text(28.0, 56.0, &format!("{index:02}"), 19, theme.accent, 400, true, "start"),
        svg_text(84.0, 56.0, &short(project.name, 48), 31, theme.text, 650, false, "start"),
        svg_text(1168.0, 55.0, "↗", 29, theme.accent, 400, false, "end"),
    ];
    for (index, line) in lines(description, 86, 2).iter().enumerate() {
        body.push(svg_text(84.0, 98.0 + index as f64 * 27.0, line, 22, theme.muted, 400, false, "start"));
    }
    let metadata = format!("{}  ·  {} stars  ·  {} forks", project.language, count(project.stars), count(project.forks));
    body.extend([
        svg_text(84.0, 162.0, &short(&metadata, 78), 17, theme.accent, 400, true, "start"),
        svg_text(1168.0, 162.0, &format!("Pushed {}", project.pushed), 16, theme.muted, 400, false, "end"),
    ]);
    document(1200, 190, project.name, &format!("{description} {metadata}."), body, theme)
}

fn display_day(value: NaiveDate) -> String {
    format!("{} {}", value.day(), value.format("%b %Y"))
}

fn activity(calendar: &Calendar, theme: &Theme) -> String {
    let start_week = calendar.first - Duration::days(calendar.first.weekday().num_days_from_sunday() as i64);
    let last_week = calendar.end - Duration::days(calendar.end.weekday().num_days_from_sunday() as i64);
    let weeks = (last_week - start_week).num_days() / 7 + 1;
    let cell = 16_f64.min((1060.0 - (weeks - 1) as f64 * 4.0) / weeks as f64);
    let step = cell + 4.0;
    let (grid_x, grid_y) = (69.0, 125.0);
    let mut body = vec![
        svg_text(28.0, 48.0, "GitHub activity", 28, theme.text, 650, false, "start"),
        svg_text(28.0, 81.0, &format!("{} — {}", display_day(calendar.first), display_day(calendar.end)), 18, theme.muted, 400, false, "start"),
        svg_text(1171.0, 48.0, &count(calendar.total), 38, theme.accent, 650, false, "end"),
        svg_text(1171.0, 78.0, "contributions / 365 days", 16, theme.muted, 400, true, "end"),
    ];
    for (weekday, label) in [(1, "Mon"), (3, "Wed"), (5, "Fri")] {
        body.push(svg_text(grid_x - 15.0, grid_y + weekday as f64 * step + cell * 0.78, label, 13, theme.muted, 400, false, "end"));
    }
    let mut lit_cells = String::new();
    let mut outline_wave = String::new();
    let mut last_month = None;
    for column in 0..weeks {
        let week_start = start_week + Duration::weeks(column);
        let month_date = week_start.max(calendar.first);
        if Some(month_date.month()) != last_month && column < weeks - 1 {
            body.push(svg_text(grid_x + column as f64 * step, grid_y - 13.0, &month_date.format("%b").to_string(), 13, theme.muted, 400, false, "start"));
            last_month = Some(month_date.month());
        }
        let mut outlines = String::new();
        for weekday in 0..7 {
            let date = week_start + Duration::days(weekday);
            let Some(item) = calendar.days.get(&date) else { continue };
            let (x, y) = (grid_x + column as f64 * step, grid_y + weekday as f64 * step);
            body.push(format!(
                "<g><title>{date}: {} contributions</title>{}</g>",
                count(item.count), rect(x, y, cell, cell, theme.levels[item.level], 3.0, None)
            ));
            if item.count > 0 {
                lit_cells.push_str(&rect(x, y, cell, cell, "#fff", 3.0, None));
            }
            outlines.push_str(&rect(x + 0.6, y + 0.6, cell - 1.2, cell - 1.2, "none", 3.0, Some(theme.accent)));
        }
        if !outlines.is_empty() {
            let begin = column as f64 * 6.0 / (weeks - 1).max(1) as f64 - 11.0;
            outline_wave.push_str(&format!(
                "<g opacity=\"0\">{outlines}<animate attributeName=\"opacity\" values=\"0;.62;0;0\" keyTimes=\"0;.08;.22;1\" begin=\"{begin:.3}s\" dur=\"10s\" repeatCount=\"indefinite\" calcMode=\"spline\" keySplines=\".4 0 .6 1;.4 0 .6 1;0 0 1 1\"/></g>"
            ));
        }
    }
    let grid_width = (weeks - 1) as f64 * step + cell;
    let sweep_width = grid_width * 0.3;
    body.push(format!(
        "<defs><clipPath id=\"activity-lit-cells\">{lit_cells}</clipPath><linearGradient id=\"activity-light\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\"><stop offset=\"0\" stop-color=\"#fff5e8\" stop-opacity=\"0\"/><stop offset=\".5\" stop-color=\"#fff5e8\" stop-opacity=\".34\"/><stop offset=\"1\" stop-color=\"#fff5e8\" stop-opacity=\"0\"/></linearGradient></defs>"
    ));
    body.push(format!(
        "<g class=\"motion\" clip-path=\"url(#activity-lit-cells)\"><rect x=\"{}\" y=\"{grid_y}\" width=\"{sweep_width}\" height=\"{}\" fill=\"url(#activity-light)\"><animate attributeName=\"x\" values=\"{};{}\" dur=\"10s\" begin=\"-1.5s\" repeatCount=\"indefinite\" calcMode=\"spline\" keySplines=\".4 0 .2 1\"/></rect></g>",
        grid_x - sweep_width, 6.0 * step + cell, grid_x - sweep_width, grid_x + grid_width
    ));
    body.push(format!("<g class=\"motion\" stroke-width=\"1.2\">{outline_wave}</g>"));
    let today_x = grid_x + (weeks - 1) as f64 * step;
    let today_y = grid_y + calendar.end.weekday().num_days_from_sunday() as f64 * step;
    let today = rect(today_x - 2.0, today_y - 2.0, cell + 4.0, cell + 4.0, "none", 5.0, Some(theme.accent));
    body.push(today.clone());
    body.push(format!(
        "<g class=\"motion\" opacity=\".18\" stroke-width=\"2.4\">{today}<animate attributeName=\"opacity\" values=\".18;.8;.18\" keyTimes=\"0;.5;1\" dur=\"6s\" repeatCount=\"indefinite\" calcMode=\"spline\" keySplines=\".4 0 .6 1;.4 0 .6 1\"/></g>"
    ));
    body.push(svg_text(947.0, 316.0, "Less", 14, theme.muted, 400, false, "end"));
    for (level, color) in theme.levels.iter().enumerate() {
        body.push(rect(964.0 + level as f64 * 21.0, 303.0, 15.0, 15.0, color, 3.0, None));
    }
    body.extend([
        svg_text(1078.0, 316.0, "More", 14, theme.muted, 400, false, "start"),
        svg_text(28.0, 316.0, "One square. One day. Your actual GitHub history.", 15, theme.muted, 400, false, "start"),
    ]);
    let description = format!(
        "{} contributions on the GitHub profile calendar from {} to {}. An outlined square marks the current day. GitHub may include anonymized private contributions enabled by the account owner.",
        count(calendar.total), calendar.first, calendar.end
    );
    document(1200, 340, "GitHub contribution calendar", &description, body, theme)
}

fn url_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in clean(value).bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn url_attribute(value: &str) -> String {
    let mut encoded = String::new();
    for byte in clean(value).bytes() {
        if byte.is_ascii_graphic() && !b"\"'<>\\`()[]".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    esc(&encoded)
}

fn picture(name: &str, alt: &str, width: &str, assets: &BTreeMap<String, String>) -> RenderResult<String> {
    let dark = assets.get(&format!("{name}-dark.svg"))
        .ok_or_else(|| invalid(format!("missing dark asset for {name}")))?;
    let light = assets.get(&format!("{name}-light.svg"))
        .ok_or_else(|| invalid(format!("missing light asset for {name}")))?;
    Ok(format!(
        "<picture>\n  <source media=\"(prefers-color-scheme: dark)\" srcset=\"{}\">\n  <img src=\"{}\" alt=\"{}\" width=\"{width}\">\n</picture>",
        esc(dark), esc(light), esc(alt)
    ))
}

fn readme(snapshot: &Snapshot<'_>, assets: &BTreeMap<String, String>) -> RenderResult<String> {
    let login = url_component(snapshot.login);
    let summary = format!(
        "{}, @{}. {} {} original public repositories, {} project stars, {} followers.",
        snapshot.name, snapshot.login, snapshot.biography, snapshot.original_repos, snapshot.total_stars, snapshot.followers
    );
    let mut body = vec![
        picture("hero", &summary, "100%", assets)?,
        String::new(),
        "<p align=\"center\">".into(),
        "  <a href=\"https://t.me/WhyFaust_tv\">".into(),
        picture("telegram", "Telegram · @WhyFaust_tv", "49%", assets)?,
        "  </a>".into(),
        format!("  <a href=\"https://github.com/{login}?tab=repositories\">"),
        picture("repositories", "Browse all GitHub repositories", "49%", assets)?,
        "  </a>".into(),
        "</p>".into(),
        String::new(),
        "## Currently building".into(),
        String::new(),
        "Latest original public projects, automatically selected from GitHub.".into(),
        String::new(),
    ];
    for (index, project) in snapshot.projects.iter().enumerate() {
        let description = if project.description.is_empty() { "Explore this project on GitHub." } else { project.description };
        let alt = format!("{}. {description} {} stars.", project.name, project.stars);
        body.extend([
            format!("<a href=\"{}\">", url_attribute(project.url)),
            picture(&format!("project-{}", index + 1), &alt, "100%", assets)?,
            "</a>".into(),
            String::new(),
        ]);
    }
    if snapshot.projects.is_empty() {
        body.extend([
            format!("<a href=\"https://github.com/{login}?tab=repositories\">Explore @{} on GitHub</a>.", esc(snapshot.login)),
            String::new(),
        ]);
    }
    body.extend([
        "## A year on GitHub".into(),
        String::new(),
        picture("activity", &format!("{} GitHub contributions over the last 365 days.", snapshot.calendar.total), "100%", assets)?,
        String::new(),
        "<p align=\"center\">".into(),
        "  <a href=\"https://t.me/WhyFaust_tv\">AI, Python, development &amp; games — @WhyFaust_tv</a>".into(),
        "</p>".into(),
        String::new(),
        "<p align=\"center\"><sub>".into(),
        format!("  From GitHub, automatically. Snapshot changed {}.<br>", snapshot.updated_at.format("%d %b %Y, %H:%M UTC")),
        format!("  <a href=\"https://github.com/{login}/{login}/actions/workflows/update-profile.yml\">Refreshes on a 5-minute schedule</a> · GitHub may delay updates."),
        "</sub></p>".into(),
        String::new(),
    ]);
    Ok(body.join("\n"))
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    match fs::read(path) {
        Ok(previous) if previous == content.as_bytes() => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(path, content)
}

pub fn render(data: &Value, root: &Path) -> Result<(), Box<dyn Error>> {
    let snapshot = Snapshot::from_json(data)?;
    let image = image_uri(root)?;
    let mut output = BTreeMap::new();
    for (name, theme) in &THEMES {
        output.insert(format!("hero-{name}.svg"), hero(&snapshot, &image, theme));
        output.insert(format!("telegram-{name}.svg"), button("Telegram", "@WhyFaust_tv", theme, true));
        output.insert(format!("repositories-{name}.svg"), button("Projects", "Browse the source on GitHub", theme, false));
        output.insert(format!("activity-{name}.svg"), activity(&snapshot.calendar, theme));
        for (index, project) in snapshot.projects.iter().enumerate() {
            output.insert(format!("project-{}-{name}.svg", index + 1), project_card(project, index + 1, theme));
        }
    }
    let login = url_component(snapshot.login);
    let assets = output.iter().map(|(filename, content)| {
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        (filename.clone(), format!("https://raw.githubusercontent.com/{login}/{login}/main/assets/generated/{filename}?v={}", &hash[..12]))
    }).collect();
    let contents = readme(&snapshot, &assets)?;
    let destination = root.join("assets/generated");
    fs::create_dir_all(&destination)?;
    for (filename, content) in output {
        write_if_changed(&destination.join(filename), &content)?;
    }
    write_if_changed(&root.join("README.md"), &contents)?;
    Ok(())
}
