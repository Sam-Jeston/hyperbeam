//! End-to-end tests: a real server, a real Postgres, real runner processes
//! driving a scripted test agent, and a fake GitHub (REST API plus a genuine git
//! smart-HTTP backend) standing in for the outside world.
//!
//! Needs Postgres. Set `HYPERBEAM_TEST_DATABASE_URL` to a server-level URL
//! whose user may create databases, e.g.
//! `postgres://hyperbeam:hyperbeam@127.0.0.1:5440/postgres`. Without it the
//! tests are skipped. Build the runner and the test agent first:
//! `cargo build -p hyperbeam-runner -p hyperbeam-test-agent`.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use base64::Engine;
use clap::Parser;
use hyperbeam_server::config::Config;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
struct ConfigArgs {
    #[command(flatten)]
    cfg: Config,
}

const UPSTREAM_TOKEN: &str = "ghp_test_upstream_token";

// ------------------------------------------------------------- fake GitHub

#[derive(Clone)]
struct FakeGithub {
    root: PathBuf,
    /// Requests the REST API received: (method, path, body).
    calls: Arc<Mutex<Vec<(String, String, Value)>>>,
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl FakeGithub {
    /// A bare repository at `<root>/acme/demo.git` with one commit on main.
    fn new(root: PathBuf) -> Self {
        let bare = root.join("acme/demo.git");
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "--bare", "--initial-branch=main", "-q"]);
        git(&bare, &["config", "http.receivepack", "true"]);
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "--initial-branch=main", "-q"]);
        std::fs::write(seed.join("README.md"), "# demo\n").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "initial"]);
        git(&seed, &["push", "-q", bare.to_str().unwrap(), "main"]);
        Self { root, calls: Arc::default() }
    }

    fn branches(&self) -> Vec<String> {
        git(&self.root.join("acme/demo.git"), &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn router(self) -> Router {
        Router::new()
            .route("/api/repos/{owner}/{repo}", get(repo_info))
            .route("/api/repos/{owner}/{repo}/pulls", post(create_pull))
            .route("/api/repos/{owner}/{repo}/pulls/{number}/merge", put(merge_pull))
            .route("/git/{*path}", any(git_backend))
            .with_state(self)
    }
}

fn authorized(headers: &HeaderMap, scheme: &str) -> bool {
    let want = match scheme {
        "bearer" => format!("Bearer {UPSTREAM_TOKEN}"),
        _ => format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{UPSTREAM_TOKEN}"))),
    };
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(want.as_str())
}

async fn repo_info(Path((owner, repo)): Path<(String, String)>, headers: HeaderMap) -> Response {
    if !authorized(&headers, "bearer") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    axum::Json(json!({ "full_name": format!("{owner}/{repo}"), "default_branch": "main", "description": "A demo repository" })).into_response()
}

async fn create_pull(State(gh): State<FakeGithub>, Path((owner, repo)): Path<(String, String)>, headers: HeaderMap, axum::Json(body): axum::Json<Value>) -> Response {
    if !authorized(&headers, "bearer") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    gh.calls.lock().unwrap().push(("POST".into(), format!("/repos/{owner}/{repo}/pulls"), body));
    (StatusCode::CREATED, axum::Json(json!({ "number": 7, "html_url": format!("https://github.example/{owner}/{repo}/pull/7") }))).into_response()
}

async fn merge_pull(State(gh): State<FakeGithub>, Path((owner, repo, number)): Path<(String, String, u64)>, axum::Json(body): axum::Json<Value>) -> Response {
    gh.calls.lock().unwrap().push(("PUT".into(), format!("/repos/{owner}/{repo}/pulls/{number}/merge"), body));
    axum::Json(json!({ "merged": true, "sha": "abc123" })).into_response()
}

/// Serve the git smart-HTTP protocol by running `git http-backend` as CGI.
async fn git_backend(
    State(gh): State<FakeGithub>,
    Path(path): Path<String>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&headers, "basic") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut child = tokio::process::Command::new("git")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", &gh.root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", format!("/{path}"))
        .env("REQUEST_METHOD", method.as_str())
        .env("QUERY_STRING", query.unwrap_or_default())
        .env("CONTENT_TYPE", headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default())
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("REMOTE_USER", "hyperbeam")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("git http-backend");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&body).await.unwrap();
    drop(stdin);
    let output = child.wait_with_output().await.unwrap().stdout;
    let split = output.windows(4).position(|w| w == b"\r\n\r\n").expect("CGI headers");
    let mut response = Response::builder();
    let mut status = 200u16;
    for line in String::from_utf8_lossy(&output[..split]).split("\r\n") {
        if let Some((name, value)) = line.split_once(": ") {
            if name.eq_ignore_ascii_case("status") {
                status = value.split(' ').next().and_then(|s| s.parse().ok()).unwrap_or(500);
            } else {
                response = response.header(name, value);
            }
        }
    }
    response.status(status).body(axum::body::Body::from(output[split + 4..].to_vec())).unwrap()
}

// ------------------------------------------------------------------ harness

struct Harness {
    base: String,
    http: reqwest::Client,
    github: FakeGithub,
    agent: PathBuf,
    _tmp: PathBuf,
}

struct Client<'a> {
    harness: &'a Harness,
    token: String,
}

impl Client<'_> {
    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = self.harness.http.request(method, format!("{}/api/v1{path}", self.harness.base)).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn ok(&self, method: Method, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(method.clone(), path, body).await;
        assert!(status.is_success(), "{method} {path} -> {status}: {value}");
        value
    }

    async fn get(&self, path: &str) -> Value {
        self.ok(Method::GET, path, None).await
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        self.ok(Method::POST, path, Some(body)).await
    }

    async fn session(&self, id: &str) -> Value {
        self.get(&format!("/sessions/{id}")).await["session"].clone()
    }

    async fn events(&self, id: &str) -> Vec<Value> {
        self.get(&format!("/sessions/{id}/events")).await["events"].as_array().cloned().unwrap_or_default()
    }

    /// Wait until the session has finished `turns` turns and is idle.
    async fn wait_turns(&self, id: &str, turns: usize) -> Vec<Value> {
        for _ in 0..300 {
            let session = self.session(id).await;
            let events = self.events(id).await;
            let ended = events.iter().filter(|e| e["kind"] == "turn_end").count();
            if ended >= turns && session["status"] == "idle" {
                return events;
            }
            assert_ne!(session["status"], "failed", "session failed: {}", session["error"]);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("session {id} did not finish {turns} turn(s): {:#?}", self.events(id).await);
    }
}

fn token_from(response: &reqwest::Response) -> String {
    let cookie = response.headers().get("set-cookie").expect("set-cookie").to_str().unwrap();
    cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string()
}

impl Harness {
    async fn start() -> Option<Harness> {
        let Ok(admin_url) = std::env::var("HYPERBEAM_TEST_DATABASE_URL") else {
            eprintln!("skipping: HYPERBEAM_TEST_DATABASE_URL is not set");
            return None;
        };
        let db_name = format!("nm_test_{}", uuid::Uuid::new_v4().simple());
        let admin = sqlx::PgPool::connect(&admin_url).await.expect("connect to test Postgres");
        sqlx::query(&format!("CREATE DATABASE {db_name}")).execute(&admin).await.unwrap();
        let mut db_url = url::Url::parse(&admin_url).unwrap();
        db_url.set_path(&db_name);

        let tmp = std::env::temp_dir().join(&db_name);
        std::fs::create_dir_all(&tmp).unwrap();
        let github = FakeGithub::new(tmp.join("github"));
        let gh_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gh_addr = gh_listener.local_addr().unwrap();
        tokio::spawn(axum::serve(gh_listener, github.clone().router()).into_future());

        let bin_dir = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
        let runner = bin_dir.join("hyperbeam-runner");
        let agent = bin_dir.join("hyperbeam-test-agent");
        for bin in [&runner, &agent] {
            assert!(bin.exists(), "build the binaries first: cargo build -p hyperbeam-runner -p hyperbeam-test-agent");
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let egress_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let egress_addr = egress_listener.local_addr().unwrap().to_string();
        let cfg = ConfigArgs::parse_from([
            "test",
            "--database-url", db_url.as_str(),
            "--master-key", &hyperbeam_server::crypto::SecretBox::generate_key(),
            "--internal-url", &base,
            "--public-url", &base,
            "--sandbox-driver", "process",
            "--runner-bin", runner.to_str().unwrap(),
            "--workspace-root", tmp.join("sandboxes").to_str().unwrap(),
            "--github-api-url", &format!("http://{gh_addr}/api"),
            "--github-git-url", &format!("http://{gh_addr}/git"),
            "--egress-addr", &egress_addr,
            "--egress-allow", "allowed.invalid",
        ])
        .cfg;
        let (app, router) = hyperbeam_server::build(cfg).await.expect("server builds");
        tokio::spawn(hyperbeam_server::egress::run(app, egress_listener));
        tokio::spawn(axum::serve(listener, router).into_future());
        Some(Harness { base, http: reqwest::Client::new(), github, agent, _tmp: tmp })
    }

    /// Set up the team and register the scripted test agent as the
    /// `scripted` harness; returns the owner's client.
    async fn owner(&self) -> Client<'_> {
        let response = self
            .http
            .post(format!("{}/api/v1/setup", self.base))
            .json(&json!({ "team_name": "Acme", "name": "Olive Owner", "email": "olive@example.com", "password": "correct horse battery" }))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{}", response.text().await.unwrap());
        let owner = Client { harness: self, token: token_from(&response) };
        owner.post("/harnesses", json!({ "name": "scripted", "title": "Scripted", "image": "unused", "command": [self.agent.to_str().unwrap()] })).await;
        owner
    }

    /// Invite and sign up a member with the given grants.
    async fn member(&self, owner: &Client<'_>, email: &str, grants: &[&str]) -> Client<'_> {
        let invite = owner.post("/invites", json!({ "email": email, "grants": grants })).await;
        let token = invite["url"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
        let response = self
            .http
            .post(format!("{}/api/v1/auth/accept-invite", self.base))
            .json(&json!({ "token": token, "name": "Mia Member", "password": "another long password" }))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{}", response.text().await.unwrap());
        Client { harness: self, token: token_from(&response) }
    }

    async fn connect_github(&self, owner: &Client<'_>) {
        owner.ok(Method::PUT, "/integrations/github", Some(json!({ "config": { "mode": "token" }, "secret": UPSTREAM_TOKEN }))).await;
        let repo = owner.post("/repos", json!({ "full_name": "acme/demo" })).await;
        assert_eq!(repo["repo"]["default_branch"], "main");
        assert_eq!(repo["repo"]["description"], "A demo repository");
    }
}

fn text_of(events: &[Value], kind: &str) -> String {
    events.iter().filter(|e| e["kind"] == kind).filter_map(|e| e["data"]["text"].as_str()).collect()
}

/// Output of the most recent tool call whose title contains `title_contains`.
/// A relaunched harness may reuse tool call ids, so updates are matched only
/// after the call itself.
fn tool_output(events: &[Value], title_contains: &str) -> String {
    let call = events
        .iter()
        .rev()
        .find(|e| e["kind"] == "tool_call" && e["data"]["title"].as_str().is_some_and(|t| t.contains(title_contains)))
        .unwrap_or_else(|| panic!("no tool call titled like {title_contains}"));
    let (id, seq) = (&call["data"]["toolCallId"], call["seq"].as_i64().unwrap());
    events
        .iter()
        .filter(|e| e["kind"] == "tool_call_update" && &e["data"]["toolCallId"] == id && e["seq"].as_i64().unwrap() > seq)
        .filter_map(|e| e["data"]["content"][0]["content"]["text"].as_str())
        .collect()
}

// -------------------------------------------------------------------- tests

#[tokio::test(flavor = "multi_thread")]
async fn agent_works_within_its_owners_permissions() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    h.connect_github(&owner).await;
    // Mia may read, push branches and open pull requests, but not merge.
    let mia = h.member(&owner, "mia@example.com", &["github:write", "github:open_pr"]).await;
    assert_eq!(mia.get("/me").await["user"]["grants"], json!(["github:read", "github:write", "github:open_pr"]));

    let prompt = r#"Add a greeting file.
!plan write the file; push; open a pull request
!say Starting on the greeting.
!sh cd demo && git status -sb | head -1 && echo hello > greeting.txt && git add . && git commit -qm "Add greeting" && git push -q origin HEAD 2>&1 && echo PUSHED
!mcp github_create_pull_request {"title": "Add greeting", "body": "Adds greeting.txt"}
!sh cd demo && git push origin HEAD:main 2>&1; echo "exit=$?"
!mcp github_merge_pull_request {"number": 7}
!mcp hyperbeam_memory_write {"key": "demo-notes", "content": "The demo repo has a greeting."}
!say All done: opened a pull request."#;
    let created = mia.post("/sessions", json!({ "prompt": prompt, "harness": "scripted", "repos": ["acme/demo"], "new_workstream": "Greeting file" })).await;
    let session = &created["session"];
    let id = session["id"].as_str().unwrap();
    let branch = session["work_branch"].as_str().unwrap().to_string();
    assert!(branch.starts_with("hyperbeam/add-a-greeting-file-"), "{branch}");
    assert_eq!(session["grants"], json!(["github:read", "github:write", "github:open_pr"]));

    let events = mia.wait_turns(id, 1).await;

    // The clone went through the proxy and landed on the work branch.
    let push = tool_output(&events, "git commit");
    assert!(push.contains(&format!("## {branch}")), "{push}");
    assert!(push.contains("PUSHED"), "{push}");
    assert!(h.github.branches().contains(&branch), "branch not on upstream: {:?}", h.github.branches());

    // Opening a pull request is allowed, attributed, and linked for the team.
    assert!(tool_output(&events, "github_create_pull_request").contains("Opened pull request #7"));
    let calls = h.github.calls.lock().unwrap().clone();
    let (_, _, pr) = calls.iter().find(|(m, p, _)| m == "POST" && p == "/repos/acme/demo/pulls").expect("pull request created upstream");
    assert_eq!(pr["head"], branch);
    assert_eq!(pr["base"], "main");
    assert!(pr["body"].as_str().unwrap().contains("on behalf of Mia Member"));

    // Pushing to the default branch and merging both need github:merge_pr.
    let denied_push = tool_output(&events, "HEAD:main");
    assert!(denied_push.contains("403") && !denied_push.contains("exit=0"), "{denied_push}");
    assert!(!git(&h.github.root.join("acme/demo.git"), &["log", "--format=%s", "main"]).contains("Add greeting"));
    assert!(tool_output(&events, "github_merge_pull_request").contains("Denied"));
    assert!(!calls.iter().any(|(m, _, _)| m == "PUT"), "merge must not reach GitHub");
    let denials: Vec<&str> = events.iter().filter(|e| e["kind"] == "gateway" && e["data"]["allowed"] == false).filter_map(|e| e["data"]["tool"].as_str()).collect();
    assert_eq!(denials, ["git push", "github_merge_pull_request"]);

    // The session, its workstream and the team's notes reflect the work.
    let session = mia.session(id).await;
    assert_eq!(session["summary"], "All done: opened a pull request.");
    assert_eq!(session["links"][0]["url"], "https://github.example/acme/demo/pull/7");
    let workstream = mia.get(&format!("/workstreams/{}", session["workstream_id"].as_str().unwrap())).await;
    assert_eq!(workstream["workstream"]["title"], "Greeting file");
    assert_eq!(workstream["workstream"]["links"][0]["kind"], "pull_request");
    assert_eq!(owner.get("/memory").await["notes"][0]["key"], "demo-notes");
    assert!(text_of(&events, "agent_message").starts_with("Starting on the greeting."));
    let plan = events.iter().find(|e| e["kind"] == "plan").expect("plan event");
    assert_eq!(plan["data"]["entries"].as_array().unwrap().len(), 3);

    // Admins can audit every attempt, allowed or not.
    let audit = owner.get("/audit?denied=true").await;
    assert_eq!(audit["audit"].as_array().unwrap().len(), 2);
    assert_eq!(mia.call(Method::GET, "/audit", None).await.0, StatusCode::FORBIDDEN);

    // A follow-up runs in the same sandbox.
    mia.post(&format!("/sessions/{id}/messages"), json!({ "text": "!sh cd demo && cat greeting.txt" })).await;
    let events = mia.wait_turns(id, 2).await;
    assert_eq!(tool_output(&events, "cat greeting.txt"), "hello");

    // Revoking a permission applies to running agents immediately.
    let mia_id = mia.get("/me").await["user"]["id"].as_str().unwrap().to_string();
    owner.ok(Method::PATCH, &format!("/users/{mia_id}"), Some(json!({ "grants": ["github:read"] }))).await;
    mia.post(&format!("/sessions/{id}/messages"), json!({ "text": "!sh cd demo && git push origin HEAD:refs/heads/sneaky 2>&1; echo \"exit=$?\"" })).await;
    let events = mia.wait_turns(id, 3).await;
    assert!(tool_output(&events, "sneaky").contains("403"));
    assert!(!h.github.branches().contains(&"sneaky".to_string()));

    // Stopping tears the sandbox down and the session can be resumed.
    mia.post(&format!("/sessions/{id}/stop"), json!({})).await;
    assert_eq!(mia.session(id).await["status"], "cancelled");
    mia.post(&format!("/sessions/{id}/messages"), json!({ "text": "!sh cd demo && git log --format=%s -1" })).await;
    let events = mia.wait_turns(id, 4).await;
    // The fresh sandbox picked the pushed branch back up.
    assert_eq!(tool_output(&events, "git log"), "Add greeting");
    assert_eq!(mia.session(id).await["launches"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn conductor_delegates_and_hears_back() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    let prompt = r#"Ship the thing.
!mcp hyperbeam_spawn_agent {"title": "Build part A", "prompt": "!say Part A is built."}
!say Delegated part A."#;
    let created = owner.post("/sessions", json!({ "prompt": prompt, "harness": "scripted", "role": "conductor" })).await;
    let id = created["session"]["id"].as_str().unwrap();

    // Turn 1: delegate. Turn 2: prompted by the platform when the worker reports.
    let events = owner.wait_turns(id, 2).await;
    assert!(tool_output(&events, "hyperbeam_spawn_agent").contains("Started worker agent"));
    let report = events
        .iter()
        .filter(|e| e["kind"] == "user_message" && e["data"]["author"] == "Hyperbeam")
        .filter_map(|e| e["data"]["text"].as_str())
        .next()
        .expect("conductor was told its worker finished");
    assert!(report.contains("Build part A") && report.contains("Part A is built."), "{report}");

    let detail = owner.get(&format!("/sessions/{id}")).await;
    let child = &detail["children"][0];
    assert_eq!(child["title"], "Build part A");
    assert_eq!(child["parent_id"], id);
    assert_eq!(child["summary"], "Part A is built.");

    // A worker cannot manage agents.
    let child_id = child["id"].as_str().unwrap();
    owner.post(&format!("/sessions/{child_id}/messages"), json!({ "text": "!mcp hyperbeam_spawn_agent {\"title\": \"x\", \"prompt\": \"y\"}" })).await;
    let events = owner.wait_turns(child_id, 2).await;
    assert!(tool_output(&events, "hyperbeam_spawn_agent").contains("Denied"));
}

#[tokio::test(flavor = "multi_thread")]
async fn only_people_who_could_have_launched_an_agent_may_direct_it() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    let mia = h.member(&owner, "mia@example.com", &[]).await;

    let created = owner.post("/sessions", json!({ "prompt": "!say hi", "harness": "scripted" })).await;
    let id = created["session"]["id"].as_str().unwrap();
    owner.wait_turns(id, 1).await;

    // Everyone can watch.
    assert_eq!(mia.events(id).await.iter().filter(|e| e["kind"] == "agent_message").count(), 1);
    assert_eq!(mia.get(&format!("/sessions/{id}")).await["can_drive"], false);
    // But the owner's agent holds grants Mia does not.
    let (status, body) = mia.call(Method::POST, &format!("/sessions/{id}/messages"), Some(json!({ "text": "do my bidding" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(mia.call(Method::POST, &format!("/sessions/{id}/stop"), Some(json!({}))).await.0, StatusCode::FORBIDDEN);

    // Members cannot change the team or its configuration.
    assert_eq!(mia.call(Method::POST, "/invites", Some(json!({ "email": "x@example.com" }))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(mia.call(Method::PUT, "/integrations/linear", Some(json!({ "secret": "k" }))).await.0, StatusCode::FORBIDDEN);
    // Setup cannot be replayed.
    let again = h.http.post(format!("{}/api/v1/setup", h.base)).json(&json!({ "team_name": "X", "name": "X", "email": "x@example.com", "password": "long enough password" })).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT);
    // Without a session cookie or key there is nothing to see.
    assert_eq!(h.http.get(format!("{}/api/v1/sessions", h.base)).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);

    // A session whose owner is removed is stopped.
    let mia_session = mia.post("/sessions", json!({ "prompt": "!sleep 30", "harness": "scripted" })).await;
    let mia_session_id = mia_session["session"]["id"].as_str().unwrap();
    let mia_id = mia.get("/me").await["user"]["id"].as_str().unwrap().to_string();
    owner.ok(Method::PATCH, &format!("/users/{mia_id}"), Some(json!({ "disabled": true }))).await;
    assert_eq!(owner.session(mia_session_id).await["status"], "cancelled");
    assert_eq!(mia.call(Method::GET, "/me", None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread")]
async fn sandbox_egress_is_limited_to_the_allowlist() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    // The harness is started with HTTPS_PROXY pointing at the egress proxy,
    // authenticated as this session.
    let prompt = "!sh curl -sS -m 10 https://blocked.invalid/ 2>&1; echo \"exit=$?\"\n\
                  !sh curl -sS -m 10 https://allowed.invalid/ 2>&1; echo \"exit=$?\"\n\
                  !sh curl -sS -m 10 -x \"http://session:nmr_forged@${HTTPS_PROXY##*@}\" https://allowed.invalid/ 2>&1; echo \"exit=$?\"";
    let created = owner.post("/sessions", json!({ "prompt": prompt, "harness": "scripted" })).await;
    let id = created["session"]["id"].as_str().unwrap();
    let events = owner.wait_turns(id, 1).await;

    // Not on the allowlist: refused by the proxy, and the team can see it.
    assert!(tool_output(&events, "blocked.invalid").contains("403"), "{}", tool_output(&events, "blocked.invalid"));
    let denied: Vec<&str> = events.iter().filter(|e| e["kind"] == "gateway" && e["data"]["allowed"] == false).filter_map(|e| e["data"]["target"].as_str()).collect();
    assert_eq!(denied, ["blocked.invalid:443"]);
    // On the allowlist: the proxy tries to connect (the name does not resolve, hence 502, not 403).
    let allowed = tool_output(&events, "-m 10 https://allowed.invalid");
    assert!(allowed.contains("502") && !allowed.contains("403"), "{allowed}");
    // A made-up credential gets nowhere.
    assert!(tool_output(&events, "nmr_forged").contains("407"), "{}", tool_output(&events, "nmr_forged"));
}

#[tokio::test(flavor = "multi_thread")]
async fn model_is_chosen_per_session_with_harness_defaults() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    let said = |events: &[Value]| text_of(events, "agent_message");
    let scripted = |harnesses: &Value| harnesses["harnesses"].as_array().unwrap().iter().find(|x| x["name"] == "scripted").unwrap().clone();
    let start = |body: Value| async { owner.post("/sessions", body).await["session"]["id"].as_str().unwrap().to_string() };

    // Nothing chosen anywhere: the harness's own default.
    let id = start(json!({ "prompt": "!model", "harness": "scripted" })).await;
    assert_eq!(said(&owner.wait_turns(&id, 1).await).trim(), "model=mock-small");

    // The harness reported what it offers; that becomes the list to pick from.
    let harness = scripted(&owner.get("/harnesses").await);
    let offered: Vec<&str> = harness["model_choices"].as_array().unwrap().iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(offered, ["mock-small", "mock-large"]);
    assert_eq!(harness["model_choices"][1]["label"], "Mock Large");

    // Chosen for one session.
    let id = start(json!({ "prompt": "!model", "harness": "scripted", "model": "mock-large" })).await;
    let events = owner.wait_turns(&id, 1).await;
    assert_eq!(said(&events).trim(), "model=mock-large");
    assert!(text_of(&events, "system").contains("model: Mock Large"));
    assert_eq!(owner.get(&format!("/sessions/{id}")).await["harness"]["model"], "Mock Large");

    // A model the harness does not have: the session still runs, and says so.
    let id = start(json!({ "prompt": "!model", "harness": "scripted", "model": "no-such-model" })).await;
    let events = owner.wait_turns(&id, 1).await;
    assert_eq!(said(&events).trim(), "model=mock-small");
    assert!(text_of(&events, "error").contains("Could not select model `no-such-model`"));

    // A default on the harness profile applies when a session does not choose.
    let harness_id = harness["id"].as_str().unwrap();
    owner.ok(Method::PATCH, &format!("/harnesses/{harness_id}"), Some(json!({ "default_model": "mock-large" }))).await;
    let id = start(json!({ "prompt": "!model", "harness": "scripted" })).await;
    assert_eq!(said(&owner.wait_turns(&id, 1).await).trim(), "model=mock-large");

    // A conductor's worker inherits the conductor's model unless told otherwise.
    let id = start(json!({
        "harness": "scripted", "role": "conductor", "model": "mock-small",
        "prompt": "!mcp hyperbeam_spawn_agent {\"title\": \"inherits\", \"prompt\": \"!model\"}\n!mcp hyperbeam_spawn_agent {\"title\": \"overrides\", \"prompt\": \"!model\", \"model\": \"mock-large\"}",
    }))
    .await;
    owner.wait_turns(&id, 3).await;
    let children = owner.get(&format!("/sessions/{id}")).await["children"].clone();
    let summary_of = |title: &str| children.as_array().unwrap().iter().find(|c| c["title"] == title).unwrap()["summary"].as_str().unwrap().trim().to_string();
    assert_eq!(summary_of("inherits"), "model=mock-small");
    assert_eq!(summary_of("overrides"), "model=mock-large");

    // An admin can restrict the choice; the default must then be one of them.
    let (status, body) = owner.call(Method::PATCH, &format!("/harnesses/{harness_id}"), Some(json!({ "models": [{ "id": "mock-small" }] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    owner.ok(Method::PATCH, &format!("/harnesses/{harness_id}"), Some(json!({ "models": [{ "id": "mock-small", "label": "Small" }], "default_model": null }))).await;
    let (status, body) = owner.call(Method::POST, "/sessions", Some(json!({ "prompt": "!model", "harness": "scripted", "model": "mock-large" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("limited to these models: mock-small"), "{body}");
    assert_eq!(scripted(&owner.get("/harnesses").await)["model_choices"], json!([{ "id": "mock-small", "label": "Small", "description": "" }]));
}

#[tokio::test(flavor = "multi_thread")]
async fn models_are_listed_as_soon_as_a_harness_has_a_credential() {
    let Some(h) = Harness::start().await else { return };
    let owner = h.owner().await;
    let find = |harnesses: &Value, name: &str| harnesses["harnesses"].as_array().unwrap().iter().find(|x| x["name"] == name).unwrap().clone();
    let settled = |name: &'static str| {
        let owner = &owner;
        async move {
            for _ in 0..150 {
                let harness = find(&owner.get("/harnesses").await, name);
                if harness["models_refreshing"] == false {
                    return harness;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            panic!("model discovery for {name} never finished");
        }
    };

    // A harness that needs a key: nothing is known, and nothing can be asked, until it has one.
    let created = owner
        .post("/harnesses", json!({
            "name": "keyed", "title": "Keyed", "image": "unused", "command": [h.agent.to_str().unwrap()],
            "credentials": [{ "env": "KEYED_API_KEY", "label": "Key", "kind": "api_key" }],
        }))
        .await;
    let id = created["harness"]["id"].as_str().unwrap().to_string();
    let before = settled("keyed").await;
    assert_eq!(before["model_choices"], json!([]));
    let (status, body) = owner.call(Method::POST, &format!("/harnesses/{id}/models/refresh"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Adding the key is enough: no session has run on this harness.
    let saved = owner.ok(Method::PUT, &format!("/harnesses/{id}/credential"), Some(json!({ "scope": "team", "env": "KEYED_API_KEY", "value": "sk-test" }))).await;
    assert_eq!(saved["models_refreshing"], true);
    let after = settled("keyed").await;
    assert_eq!(after["models_error"], Value::Null, "{after}");
    let offered: Vec<&str> = after["model_choices"].as_array().unwrap().iter().filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(offered, ["mock-small", "mock-large"]);
    assert_eq!(owner.get("/sessions").await["sessions"], json!([]), "discovery must not appear as a session");

    // A harness that cannot start reports why, and keeps the list it had.
    owner.ok(Method::PATCH, &format!("/harnesses/{id}"), Some(json!({ "command": ["/nonexistent/agent"] }))).await;
    let broken = settled("keyed").await;
    assert!(broken["models_error"].as_str().is_some_and(|e| !e.is_empty()), "{broken}");
    assert_eq!(broken["model_choices"].as_array().unwrap().len(), 2);

    // On request, for a harness that needs no key.
    owner.ok(Method::POST, &format!("/harnesses/{}/models/refresh", find(&owner.get("/harnesses").await, "scripted")["id"].as_str().unwrap()), None).await;
    let scripted = settled("scripted").await;
    assert_eq!(scripted["model_choices"].as_array().unwrap().len(), 2, "{scripted}");
}
