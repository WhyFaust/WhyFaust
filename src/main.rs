mod collect;
mod render;

use serde_json::Value;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

struct Options {
    username: String,
    root: PathBuf,
    fetch_only: bool,
    render_only: bool,
}

fn options() -> Result<Option<Options>, Box<dyn Error>> {
    let mut options = Options {
        username: env::var("GITHUB_REPOSITORY_OWNER").unwrap_or_else(|_| "WhyFaust".to_owned()),
        root: env::current_dir()?,
        fetch_only: false,
        render_only: false,
    };
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--username" => options.username = args.next().ok_or("--username needs a value")?,
            "--root" => options.root = PathBuf::from(args.next().ok_or("--root needs a path")?),
            "--fetch-only" => options.fetch_only = true,
            "--render-only" => options.render_only = true,
            "--help" | "-h" => {
                println!("whyfaust-profile [--username LOGIN] [--root PATH] [--fetch-only | --render-only]");
                return Ok(None);
            }
            _ => return Err(format!("Unknown argument: {argument}").into()),
        }
    }
    if options.fetch_only && options.render_only {
        return Err("--fetch-only and --render-only cannot be combined".into());
    }
    Ok(Some(options))
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let destination = options.root.join("data/profile.json");
    let previous: Option<Value> = match fs::read_to_string(&destination) {
        Ok(content) => Some(serde_json::from_str(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if previous.as_ref().is_some_and(|value| !value.is_object()) {
        return Err("Existing snapshot is not an object".into());
    }
    if options.render_only {
        render::render(previous.as_ref().ok_or("No snapshot found for --render-only")?, &options.root)?;
        println!("Profile artwork regenerated from the saved snapshot");
        return Ok(());
    }
    let data = collect::snapshot_with_timestamp(collect::collect_profile(&options.username)?, previous.as_ref());
    if !options.fetch_only {
        render::render(&data, &options.root)?;
    }
    fs::create_dir_all(options.root.join("data"))?;
    let temporary = destination.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_string_pretty(&data)? + "\n")?;
    fs::rename(temporary, destination)?;
    println!("Profile refreshed: {}", options.username);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Profile refresh failed: {error}");
            ExitCode::FAILURE
        }
    }
}
