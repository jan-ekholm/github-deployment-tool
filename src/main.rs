use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, exit};
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
    webhook_secret: String,
}

const VERSION: &str = "1.0";

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

    println!(
        "building succeeded for repo {}, script: {}",
        config.repo_name,
        config.build_script.display()
    );

    Ok(())
}

fn validate_signature(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(hex_sig) = header.and_then(|h| h.strip_prefix("sha256=")) else {
        return false;
    };

    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
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
    println!("listening for incoming GitHub events");

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

            if !validate_signature(
                config.webhook_secret.as_bytes(),
                &body,
                header(&request, "X-Hub-Signature-256"),
            ) {
                eprintln!("rejected request with bad signature");
                let _ =
                    request.respond(Response::from_string("bad signature").with_status_code(401));
                continue;
            }

            match header(&request, "X-GitHub-Event") {
                Some("push") => {}
                Some("ping") => {
                    println!("received a ping, sending pong");
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

            println!("push event received for {}, branch: {}", event.repository.full_name, event.git_ref);

            if event.repository.full_name != config.repo_name {
                println!(
                    "wrong repository name: {}, expected: {}",
                    event.repository.full_name, config.repo_name
                );
                let _ = request.respond(Response::from_string("not our repository"));
                continue;
            }

            let wanted_ref = format!("refs/heads/{}", config.branch_name);
            if event.git_ref != wanted_ref {
                println!(
                    "wrong branch name: {}, expected: {}",
                    event.git_ref, wanted_ref
                );
                let _ = request.respond(Response::from_string("not our branch"));
                continue;
            }

            // got this far so the push was fine, let GitHub know
            _ = request.respond(Response::new_empty(tiny_http::StatusCode(200)));
            println!("sent response: 200 ok");

            // do the build. It can fail but a failure to build does not affect the status code we
            // send back to GitHub
            build(&config)?;
        }
    }

    // never really reached
    Ok(())
}

fn usage() {
    let app_name = std::env::args().next().unwrap();
    println!("Usage: {} options", app_name);
    println!("Options:");
    println!("    --port <port>           - the port number to bind to");
    println!("    --repo_name <repo>      - the name of the repository to watch");
    println!("    --branch <branch>       - the name of the branch in the repository");
    println!("    --build_script <script> - full path to the script that performs the build");
    println!("    --help                  - this help text");
    println!();
    println!("Additionally the environment variable WEBHOOK_SECRET must be set");

    exit(0)
}

fn parse_commandline() -> Result<HashMap<String, String>, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            usage()
        }

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

    let webhook_secret = match std::env::var("WEBHOOK_SECRET") {
        Ok(s) => s,
        Err(_) => return Err("WEBHOOK_SECRET environment variable not set".to_string()),
    };

    println!("incoming port: {}", port);
    println!("repository:    {}", repo_name);
    println!("branch name:   {}", branch_name);
    println!("build script:  {}", build_script.display());

    Ok(Config {
        repo_name,
        port,
        branch_name,
        build_script,
        webhook_secret,
    })
}

fn main() {
    println!("GitHub deployment tool {}", VERSION);

    match parse_config() {
        Ok(config) => match serve(&config) {
            Err(e) => eprintln!("server error: {e}"),
            _ => {}
        },
        Err(error) => {
            println!("Failed to setup config: {error}");
        }
    }
}
