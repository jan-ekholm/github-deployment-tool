use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tiny_http::{Response, Server};

#[derive(Deserialize)]
struct PushEvent {
    #[serde(rename = "ref")] // `ref` is a Rust keyword
    git_ref: String,
    after: String,
    repository: Repository,
}

#[derive(Deserialize)]
struct Repository {
    full_name: String,
}

#[derive(Debug)]
pub struct Config {
    port: u16,
    repo_name: String,
    branch_name: String,
    build_script: PathBuf,
}

fn build(config: &Config) -> Result<(), String> {
    println!(
        "building repo {}, script: {}",
        config.repo_name,
        config.build_script.display()
    );

    let status = Command::new(&config.build_script)
        .status()
        .map_err(|e| format!("failed to run build script: {e}"))?;

    if !status.success() {
        return Err(format!(
            "build of {} failed failed: {status}",
            config.build_script.display()
        ));
    }

    Ok(())
}

fn header<'a>(request: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn serve(config: &Config) -> Result<(), String> {
    let server = Server::http(("0.0.0.0", config.port)).map_err(|e| e.to_string())?;
    println!(
        "listening for incoming GitHub events on port {}",
        config.port
    );

    loop {
        for mut request in server.incoming_requests() {
            let mut body = Vec::new();
            if let Err(e) = request.as_reader().read_to_end(&mut body) {
                eprintln!("failed to read body: {e}");
                continue;
            }

            println!(
                "{} {} ({} bytes)",
                request.method(),
                request.url(),
                body.len()
            );

            match header(&request, "X-GitHub-Event") {
                Some("push") => {}
                Some("ping") => {
                    let _ = request.respond(Response::from_string("pong"));
                    continue;
                }
                _ => {
                    let _ = request.respond(Response::from_string("ignored"));
                    continue;
                }
            }

            let event: PushEvent = match serde_json::from_slice(&body) {
                Ok(e) => e,
                Err(e) => {
                    let _ = request.respond(
                        Response::from_string(format!("bad json: {e}")).with_status_code(400),
                    );
                    continue;
                }
            };

            println!(
                "push event received for {}, ref: {}",
                event.repository.full_name, event.git_ref
            );

            let wanted_ref = format!("refs/heads/{}", config.branch_name);
            if event.repository.full_name != config.repo_name || event.git_ref != wanted_ref {
                let _ = request.respond(Response::from_string("not our branch"));
                continue;
            }

            build(&config)?;
        }
    }

    // never really reached
    Ok(())
}

fn parse_commandline() -> Result<HashMap<String, String>, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        let key = arg
            .strip_prefix("--")
            .ok_or_else(|| format!("expected --key, got {arg:?}"))?;

        let value = args
            .next()
            .ok_or_else(|| format!("--{key} needs a value"))?;
        map.insert(key.to_string(), value);
    }

    Ok(map)
}

fn parse_config() -> Result<Config, String> {
    let mut args = parse_commandline()?;

    let repo_name = args
        .remove("repo_name")
        .ok_or("--repo_name <name> is required")?;

    let branch_name = args
        .remove("branch")
        .ok_or("--branch <branch name> is required")?;

    let build_script = PathBuf::from(
        args.remove("build_script")
            .ok_or("--build_script <path> is required")?,
    );

    let port: u16 = args
        .remove("port")
        .ok_or("--port <number> is required")?
        .parse()
        .map_err(|e| format!("--port: {e}"))?;

    // the build script must exist
    let build_script = build_script
        .canonicalize()
        .map_err(|e| format!("--build_script {}: {e}", build_script.display()))?;

    Ok(Config {
        repo_name,
        port,
        branch_name,
        build_script,
    })
}

fn main() {
    println!("GitHub deployment tool!");

    match parse_config() {
        Ok(config) => match serve(&config) {
            Err(e) => eprintln!("server error: {e}"),
            _ => {}
        },
        Err(error) => {
            println!("Failed to parse commandline: {error}");
        }
    }
}
