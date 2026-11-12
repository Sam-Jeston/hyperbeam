//! `hyperbeam`: start, follow and steer agents from a terminal.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hyperbeam", version, about = "Command-line client for a Hyperbeam server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save the server address and an API key (create one under Settings → My account).
    Login {
        /// Server URL, e.g. https://hyperbeam.example.com
        #[arg(long)]
        server: String,
        /// API key. Read from HYPERBEAM_API_KEY or prompted for when omitted.
        #[arg(long, env = "HYPERBEAM_API_KEY", hide_env_values = true)]
        key: Option<String>,
    },
    /// Show who you are signed in as.
    Whoami,
    /// Start an agent.
    Run {
        /// What should get done.
        prompt: String,
        /// Repository to check out (owner/name). Repeatable.
        #[arg(long = "repo")]
        repos: Vec<String>,
        /// Skill to install. Repeatable.
        #[arg(long = "skill")]
        skills: Vec<String>,
        /// Harness profile; defaults to the team's.
        #[arg(long)]
        harness: Option<String>,
        /// Model to run on; defaults to the harness profile's.
        #[arg(long, short)]
        model: Option<String>,
        /// Start a conductor that delegates to worker agents.
        #[arg(long)]
        conductor: bool,
        /// Put it on the board as a new workstream with this title.
        #[arg(long)]
        workstream: Option<String>,
        /// Limit the agent to these permissions (e.g. github:read). Repeatable.
        /// Defaults to everything you hold.
        #[arg(long = "grant")]
        grants: Option<Vec<String>>,
        /// Stream the agent's work until its turn ends.
        #[arg(long, short)]
        follow: bool,
    },
    /// Run a saved workflow.
    Workflow {
        /// Workflow name.
        name: String,
        /// Input as name=value. Repeatable.
        #[arg(long = "input", short)]
        inputs: Vec<String>,
        /// Model to run on; overrides the workflow's.
        #[arg(long, short)]
        model: Option<String>,
        #[arg(long)]
        workstream: Option<String>,
        #[arg(long, short)]
        follow: bool,
    },
    /// List agent sessions.
    Ls {
        /// Include sessions that are no longer running.
        #[arg(long, short)]
        all: bool,
        /// Only sessions you started.
        #[arg(long)]
        mine: bool,
    },
    /// Print a session's log.
    Logs {
        id: String,
        /// Keep streaming as the agent works.
        #[arg(long, short)]
        follow: bool,
    },
    /// Send a follow-up message to a session.
    Send {
        id: String,
        text: String,
        #[arg(long, short)]
        follow: bool,
    },
    /// Interrupt the turn in progress, leaving the session alive.
    Interrupt { id: String },
    /// Stop a session and tear down its sandbox.
    Stop { id: String },
}

#[derive(Serialize, Deserialize)]
struct Config {
    server: String,
    key: String,
}

fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| anyhow!("cannot find a home directory for the config file"))?;
    Ok(base.join("hyperbeam").join("config.json"))
}

struct Client {
    http: reqwest::Client,
    server: String,
    key: String,
}

impl Client {
    fn load() -> Result<Self> {
        // Environment wins, so CI needs no config file.
        let from_env = std::env::var("HYPERBEAM_SERVER").ok().zip(std::env::var("HYPERBEAM_API_KEY").ok());
        let (server, key) = match from_env {
            Some(pair) => pair,
            None => {
                let path = config_path()?;
                let raw = std::fs::read_to_string(&path).map_err(|_| anyhow!("not signed in: run `hyperbeam login --server <url>` first"))?;
                let cfg: Config = serde_json::from_str(&raw).with_context(|| format!("{} is corrupt", path.display()))?;
                (cfg.server, cfg.key)
            }
        };
        Ok(Self { http: reqwest::Client::new(), server: server.trim_end_matches('/').to_string(), key })
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.http.request(method, format!("{}/api/v1{path}", self.server)).bearer_auth(&self.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.with_context(|| format!("could not reach {}", self.server))?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("{}", value["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("the server answered {status}")));
        }
        Ok(value)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.call(reqwest::Method::GET, path, None).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// Accept a full session id or an unambiguous prefix of one.
    async fn resolve(&self, id: &str) -> Result<String> {
        if id.len() == 36 {
            return Ok(id.to_string());
        }
        let sessions = self.get("/sessions?limit=500").await?;
        let matches: Vec<&str> = sessions["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["id"].as_str())
            .filter(|full| full.starts_with(id) || full.replace('-', "").ends_with(id))
            .collect();
        match matches.as_slice() {
            [one] => Ok(one.to_string()),
            [] => bail!("no session matches `{id}`"),
            _ => bail!("`{id}` matches {} sessions; use more characters", matches.len()),
        }
    }

    /// Print a session's events from `after` onwards, stopping as `until` says.
    async fn stream(&self, id: &str, until: Until, after: i64) -> Result<()> {
        let response = self
            .http
            .get(format!("{}/api/v1/sessions/{id}/stream?after={after}", self.server))
            .bearer_auth(&self.key)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("the server answered {}", response.status());
        }
        let mut printer = Printer::default();
        let mut caught_up = false;
        let mut bytes = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = bytes.next().await {
            buffer.push_str(&String::from_utf8_lossy(&chunk?));
            while let Some(end) = buffer.find("\n\n") {
                let frame: String = buffer.drain(..end + 2).collect();
                let mut name = "";
                let mut data = String::new();
                for line in frame.lines() {
                    if let Some(value) = line.strip_prefix("event:") {
                        name = value.trim();
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push_str(value.trim_start());
                    }
                }
                match name {
                    "event" => {
                        let Ok(event) = serde_json::from_str::<Value>(&data) else { continue };
                        let stop = printer.print(&event);
                        let done = match (until, stop) {
                            (Until::TurnEnds, Some(_)) => true,
                            // History is full of ended turns; only an end seen live counts.
                            (Until::SessionEnds, Some(Stop::Session)) => caught_up,
                            _ => false,
                        };
                        if done {
                            return Ok(());
                        }
                    }
                    "caught_up" => {
                        caught_up = true;
                        printer.end_line();
                        let ended = || async {
                            let status = self.get(&format!("/sessions/{id}")).await.ok()?["session"]["status"].as_str().map(str::to_string)?;
                            Some(!matches!(status.as_str(), "pending" | "starting" | "running" | "idle"))
                        };
                        if until == Until::CaughtUp || (until == Until::SessionEnds && ended().await == Some(true)) {
                            return Ok(());
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Until {
    /// Print what exists and return.
    CaughtUp,
    /// Return when the next turn ends (or the session does).
    TurnEnds,
    /// Keep following until the session's sandbox is gone.
    SessionEnds,
}

enum Stop {
    Turn,
    Session,
}

/// Renders the event log as readable terminal output.
#[derive(Default)]
struct Printer {
    mid_line: bool,
}

impl Printer {
    fn end_line(&mut self) {
        if self.mid_line {
            println!();
            self.mid_line = false;
        }
    }

    fn line(&mut self, text: &str) {
        self.end_line();
        println!("{text}");
    }

    /// Prints one event; reports when a turn or the session ended.
    fn print(&mut self, event: &Value) -> Option<Stop> {
        let data = &event["data"];
        match event["kind"].as_str().unwrap_or_default() {
            "user_message" => {
                self.line(&format!("\n> {} — {}", data["author"].as_str().unwrap_or("someone"), first_line(data["text"].as_str().unwrap_or(""))));
            }
            "agent_message" => {
                if let Some(text) = data["text"].as_str() {
                    print!("{text}");
                    std::io::stdout().flush().ok();
                    self.mid_line = !text.ends_with('\n');
                }
            }
            "tool_call" => self.line(&format!("  ⚙ {}", data["title"].as_str().unwrap_or("tool"))),
            "tool_call_update" if data["status"] == "failed" => self.line("    ✗ failed"),
            "gateway" => {
                let mark = if data["allowed"] == false { "⛔" } else { "✓" };
                self.line(&format!("  {mark} {}", data["summary"].as_str().unwrap_or("")));
            }
            "question" => {
                self.line(&format!("  ? {}", data["text"].as_str().unwrap_or("")));
                for option in data["options"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    self.line(&format!("      - {option}"));
                }
            }
            "system" => self.line(&format!("  · {}", data["text"].as_str().unwrap_or(""))),
            "error" => self.line(&format!("  ✗ {}", data["text"].as_str().unwrap_or(""))),
            "turn_end" => {
                self.end_line();
                return Some(Stop::Turn);
            }
            "status" => {
                let status = data["status"].as_str().unwrap_or("");
                if matches!(status, "failed" | "cancelled" | "completed") {
                    self.line(&format!("  · session {status}"));
                    return Some(Stop::Session);
                }
            }
            _ => {}
        }
        None
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if line.chars().count() > 100 { format!("{}…", line.chars().take(99).collect::<String>()) } else { line.to_string() }
}

fn short(id: &str) -> &str {
    // UUIDv7 ids share a time prefix; the tail is what tells them apart.
    &id[id.len().saturating_sub(8)..]
}

async fn started(client: &Client, session: &Value, follow: bool) -> Result<()> {
    let id = session["id"].as_str().unwrap_or_default();
    println!("Started “{}”", session["title"].as_str().unwrap_or(""));
    println!("{}/s/{id}", client.server);
    if follow {
        client.stream(id, Until::TurnEnds, 0).await?;
    } else {
        println!("Follow with: hyperbeam logs -f {}", short(id));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    // Die quietly when piped into `head`, like other command-line tools.
    #[cfg(unix)]
    // SAFETY: restoring the default disposition of SIGPIPE before any output.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Login { server, key } => {
            let key = match key {
                Some(key) => key,
                None => {
                    eprint!("API key: ");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line.trim().to_string()
                }
            };
            let client = Client { http: reqwest::Client::new(), server: server.trim_end_matches('/').to_string(), key };
            let me = client.get("/me").await.context("that server and key did not work")?;
            let path = config_path()?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, serde_json::to_string_pretty(&Config { server: client.server.clone(), key: client.key.clone() })?)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            println!("Signed in as {} ({}).", me["user"]["name"].as_str().unwrap_or(""), me["user"]["email"].as_str().unwrap_or(""));
        }
        Command::Whoami => {
            let client = Client::load()?;
            let me = client.get("/me").await?;
            let user = &me["user"];
            println!("{} <{}> · {} on {}", user["name"].as_str().unwrap_or(""), user["email"].as_str().unwrap_or(""), user["role"].as_str().unwrap_or(""), client.server);
            let grants: Vec<&str> = user["grants"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            println!("Your agents may: {}", if grants.is_empty() { "nothing outside their sandbox".to_string() } else { grants.join(", ") });
        }
        Command::Run { prompt, repos, skills, harness, model, conductor, workstream, grants, follow } => {
            let client = Client::load()?;
            let mut body = json!({
                "prompt": prompt,
                "repos": repos,
                "skills": skills,
                "role": if conductor { "conductor" } else { "worker" },
            });
            if let Some(harness) = harness {
                body["harness"] = json!(harness);
            }
            if let Some(model) = model {
                body["model"] = json!(model);
            }
            if let Some(title) = workstream {
                body["new_workstream"] = json!(title);
            }
            if let Some(grants) = grants {
                body["grants"] = json!(grants);
            }
            let created = client.post("/sessions", body).await?;
            started(&client, &created["session"], follow).await?;
        }
        Command::Workflow { name, inputs, model, workstream, follow } => {
            let client = Client::load()?;
            let workflows = client.get("/workflows").await?;
            let workflow = workflows["workflows"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|w| w["name"] == name.as_str())
                .ok_or_else(|| anyhow!("no workflow named `{name}`"))?;
            let mut values = BTreeMap::new();
            for input in inputs {
                let (key, value) = input.split_once('=').ok_or_else(|| anyhow!("write inputs as name=value, got `{input}`"))?;
                values.insert(key.to_string(), value.to_string());
            }
            let mut body = json!({ "inputs": values });
            if let Some(model) = model {
                body["model"] = json!(model);
            }
            if let Some(title) = workstream {
                body["new_workstream"] = json!(title);
            }
            let created = client.post(&format!("/workflows/{}/run", workflow["id"].as_str().unwrap_or_default()), body).await?;
            started(&client, &created["session"], follow).await?;
        }
        Command::Ls { all, mine } => {
            let client = Client::load()?;
            let sessions = client.get(&format!("/sessions?live={}&mine={mine}", !all)).await?;
            let list = sessions["sessions"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("{}", if all { "No sessions yet." } else { "No agents are running. Use --all to see past sessions." });
            }
            for session in list {
                // An open question is what the agent needs next, whatever its state.
                let question = session["question"]["text"].as_str().map(|q| format!("asks: {q}"));
                let headline = match &question {
                    Some(question) => Some(question.as_str()),
                    None if session["status"] == "running" => session["headline"].as_str(),
                    None => session["summary"].as_str(),
                };
                println!(
                    "{}  {:<9} {}{}",
                    short(session["id"].as_str().unwrap_or("")),
                    if question.is_some() { "asking" } else { session["status"].as_str().unwrap_or("") },
                    first_line(session["title"].as_str().unwrap_or("")),
                    headline.map(first_line).filter(|h| !h.is_empty()).map(|h| format!("\n          {h}")).unwrap_or_default(),
                );
            }
        }
        Command::Logs { id, follow } => {
            let client = Client::load()?;
            let id = client.resolve(&id).await?;
            client.stream(&id, if follow { Until::SessionEnds } else { Until::CaughtUp }, 0).await?;
        }
        Command::Send { id, text, follow } => {
            let client = Client::load()?;
            let id = client.resolve(&id).await?;
            let seq = client.get(&format!("/sessions/{id}")).await?["session"]["event_seq"].as_i64().unwrap_or(0);
            client.post(&format!("/sessions/{id}/messages"), json!({ "text": text })).await?;
            if follow {
                client.stream(&id, Until::TurnEnds, seq).await?;
            } else {
                println!("Sent.");
            }
        }
        Command::Interrupt { id } => {
            let client = Client::load()?;
            let id = client.resolve(&id).await?;
            let result = client.post(&format!("/sessions/{id}/cancel"), json!({})).await?;
            println!("{}", if result["interrupted"] == true { "Interrupted." } else { "Nothing was running." });
        }
        Command::Stop { id } => {
            let client = Client::load()?;
            let id = client.resolve(&id).await?;
            client.post(&format!("/sessions/{id}/stop"), json!({})).await?;
            println!("Stopped.");
        }
    }
    Ok(())
}
