//! Stand-ins for Slack and Discord: the HTTP API each bot calls, and the
//! WebSocket each bot dials out to (Socket Mode, the Gateway). Tests push
//! what people do down the socket and read back what the bot called.

#![allow(dead_code)]

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

pub const SLACK_BOT_TOKEN: &str = "xoxb-test-bot-token";
pub const SLACK_APP_TOKEN: &str = "xapp-test-app-token";
pub const SLACK_BOT: &str = "UBOT";
pub const DISCORD_TOKEN: &str = "discord-test-bot-token";
pub const DISCORD_BOT: &str = "999";

/// Poll until `check` returns something, or give up after a few seconds.
pub async fn eventually<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    for _ in 0..300 {
        if let Some(found) = check() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

type Socket = Arc<Mutex<Option<mpsc::UnboundedSender<Value>>>>;

/// Forward what a test pushes to the connected bot, and hand what the bot
/// sends to `on_frame`.
async fn pump(socket: WebSocket, slot: Socket, connections: Arc<AtomicU64>, greeting: Value, on_frame: impl Fn(Value) -> Option<Value> + Send + 'static) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    sink.send(Message::Text(greeting.to_string().into())).await.ok();
    let replies = tx.clone();
    *slot.lock().unwrap() = Some(tx);
    connections.fetch_add(1, Ordering::SeqCst);
    let reader = tokio::spawn(async move {
        while let Some(Ok(frame)) = stream.next().await {
            if let Message::Text(text) = frame
                && let Ok(value) = serde_json::from_str::<Value>(text.as_str())
                && let Some(reply) = on_frame(value)
            {
                replies.send(reply).ok();
            }
        }
    });
    while let Some(value) = rx.recv().await {
        if sink.send(Message::Text(value.to_string().into())).await.is_err() {
            break;
        }
    }
    reader.abort();
}

// ------------------------------------------------------------------- Slack

#[derive(Clone, Default)]
pub struct FakeSlack {
    pub base: Arc<Mutex<String>>,
    /// Web API calls the bot made: (method, body).
    pub calls: Arc<Mutex<Vec<(String, Value)>>>,
    /// Envelopes the bot acknowledged.
    pub acks: Arc<Mutex<Vec<String>>>,
    /// What `conversations.replies` returns.
    pub replies: Arc<Mutex<Vec<Value>>>,
    /// How many times the bot has connected.
    pub connections: Arc<AtomicU64>,
    socket: Socket,
    counter: Arc<AtomicU64>,
}

impl FakeSlack {
    /// Serve on a free port; returns the Web API's base URL.
    pub async fn start(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        *self.base.lock().unwrap() = base.clone();
        let router = Router::new().route("/api/{method}", post(slack_api)).route("/socket", get(slack_socket)).with_state(self.clone());
        tokio::spawn(axum::serve(listener, router).into_future());
        format!("{base}/api")
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub async fn connected(&self) {
        eventually("the bot to open its Socket Mode connection", || self.socket.lock().unwrap().as_ref().map(|_| ())).await;
    }

    fn push(&self, kind: &str, payload: Value) -> String {
        let id = format!("env-{}", self.next());
        let envelope = json!({ "envelope_id": id, "type": kind, "payload": payload, "accepts_response_payload": false });
        self.socket.lock().unwrap().as_ref().expect("bot is connected").send(envelope).unwrap();
        id
    }

    /// Someone writes a message. Returns its timestamp, which is its id.
    pub fn say(&self, user: &str, channel: &str, thread: Option<&str>, text: &str) -> String {
        self.say_with(user, channel, thread, text, json!({}))
    }

    /// As [`say`], with extra fields on the event or its envelope.
    pub fn say_with(&self, user: &str, channel: &str, thread: Option<&str>, text: &str, extra: Value) -> String {
        let ts = format!("1700000000.{:06}", self.next());
        let mut event = json!({
            "type": "message", "user": user, "text": text, "ts": ts, "channel": channel,
            "channel_type": if channel.starts_with('D') { "im" } else { "channel" },
        });
        if let Some(thread) = thread {
            event["thread_ts"] = json!(thread);
        }
        let mut payload = json!({ "team_id": "T1", "type": "event_callback" });
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            if key == "is_ext_shared_channel" {
                payload[key] = value;
            } else {
                event[key] = value;
            }
        }
        payload["event"] = event;
        self.push("events_api", payload);
        ts
    }

    /// Someone presses a button on one of the bot's messages.
    pub fn click(&self, user: &str, channel: &str, message: &str, thread: &str, value: &str) {
        self.push(
            "interactive",
            json!({
                "type": "block_actions",
                "user": { "id": user, "name": user.to_lowercase() },
                "container": { "channel_id": channel, "message_ts": message, "thread_ts": thread },
                "actions": [{ "action_id": "b0", "value": value, "type": "button" }],
            }),
        );
    }

    pub fn calls_to(&self, method: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|(m, _)| m == method).map(|(_, body)| body.clone()).collect()
    }

    /// The Markdown of a message the bot sent.
    pub fn text_of(body: &Value) -> String {
        body["blocks"][0]["text"].as_str().or(body["text"].as_str()).unwrap_or_default().to_string()
    }

    /// The values of the buttons on a message the bot sent.
    pub fn buttons_of(body: &Value) -> Vec<(String, String)> {
        body["blocks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "actions")
            .flat_map(|block| block["elements"].as_array().cloned().unwrap_or_default())
            .map(|b| (b["text"]["text"].as_str().unwrap_or_default().to_string(), b["value"].as_str().unwrap_or_default().to_string()))
            .collect()
    }

    /// Wait for a call to `method` whose body satisfies `matches`.
    pub async fn wait_for(&self, method: &str, matches: impl Fn(&Value) -> bool) -> Value {
        eventually(&format!("a matching {method} call"), || self.calls_to(method).into_iter().find(|body| matches(body))).await
    }
}

/// The people of the fake workspace.
fn slack_member(id: &str) -> Value {
    let (name, email, team, stranger) = match id {
        "UOLIVE" => ("Olive Owner", "Olive@Example.com", "T1", false),
        "UMIA" => ("Mia Member", "mia@example.com", "T1", false),
        "UNOBODY" => ("Nora Nobody", "nora@elsewhere.example", "T1", false),
        // A guest from another organisation claiming the owner's address.
        "UGUEST" => ("Olive Impostor", "olive@example.com", "T2", true),
        _ => ("Someone", "", "T1", false),
    };
    json!({
        "id": id, "team_id": team, "name": name.to_lowercase().replace(' ', "."), "real_name": name,
        "is_bot": false, "deleted": false, "is_stranger": stranger, "is_email_confirmed": true,
        "profile": { "display_name": "", "email": email },
    })
}

async fn slack_api(State(slack): State<FakeSlack>, Path(method): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let json_body = headers.get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
    let body: Value = if json_body {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    } else {
        Value::Object(url::form_urlencoded::parse(&body).map(|(k, v)| (k.into_owned(), Value::String(v.into_owned()))).collect())
    };
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().trim_start_matches("Bearer ").to_string();
    let wanted = if method == "apps.connections.open" { SLACK_APP_TOKEN } else { SLACK_BOT_TOKEN };
    if token != wanted {
        return axum::Json(json!({ "ok": false, "error": "invalid_auth" })).into_response();
    }
    let answer = match method.as_str() {
        "auth.test" => json!({ "ok": true, "user_id": SLACK_BOT, "user": "hyperbeam", "team_id": "T1", "team": "Acme" }),
        "apps.connections.open" => json!({ "ok": true, "url": format!("{}/socket", slack.base.lock().unwrap().replace("http://", "ws://")) }),
        "chat.postMessage" => json!({ "ok": true, "ts": format!("1700000001.{:06}", slack.next()) }),
        "users.info" => json!({ "ok": true, "user": slack_member(body["user"].as_str().unwrap_or_default()) }),
        "conversations.replies" => json!({ "ok": true, "messages": *slack.replies.lock().unwrap() }),
        "users.conversations" => json!({
            "ok": true,
            "channels": [
                { "id": "CDEV", "name": "dev", "is_private": false },
                { "id": "CPARTNERS", "name": "partners", "is_private": false, "is_ext_shared": true },
            ],
            "response_metadata": { "next_cursor": "" },
        }),
        _ => json!({ "ok": true }),
    };
    // Recorded with the id Slack gave the new message, as `_ts`.
    let mut recorded = body;
    if let (Some(ts), Some(fields)) = (answer.get("ts"), recorded.as_object_mut()) {
        fields.insert("_ts".into(), ts.clone());
    }
    slack.calls.lock().unwrap().push((method, recorded));
    axum::Json(answer).into_response()
}

async fn slack_socket(State(slack): State<FakeSlack>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        let acks = slack.acks.clone();
        pump(socket, slack.socket.clone(), slack.connections.clone(), json!({ "type": "hello" }), move |frame| {
            if let Some(id) = frame["envelope_id"].as_str() {
                acks.lock().unwrap().push(id.to_string());
            }
            None
        })
        .await;
    })
}

// ----------------------------------------------------------------- Discord

#[derive(Clone, Default)]
pub struct FakeDiscord {
    pub base: Arc<Mutex<String>>,
    /// REST calls the bot made: (method, path, body).
    pub calls: Arc<Mutex<Vec<(String, String, Value)>>>,
    /// What the bot identified with.
    pub identified: Arc<Mutex<Option<Value>>>,
    /// Refuse direct messages, as a person with them closed does.
    pub dms_closed: Arc<Mutex<bool>>,
    /// How many times the bot has connected.
    pub connections: Arc<AtomicU64>,
    socket: Socket,
    counter: Arc<AtomicU64>,
}

impl FakeDiscord {
    /// Serve on a free port; returns the REST API's base URL.
    pub async fn start(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        *self.base.lock().unwrap() = base.clone();
        let router = Router::new().route("/gateway", get(discord_gateway)).route("/api/{*path}", any(discord_api)).with_state(self.clone());
        tokio::spawn(axum::serve(listener, router).into_future());
        format!("{base}/api")
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Wait until the bot has identified and been told it is ready.
    pub async fn connected(&self) {
        eventually("the bot to identify on the Gateway", || self.identified.lock().unwrap().as_ref().map(|_| ())).await;
    }

    fn dispatch(&self, kind: &str, d: Value) {
        let frame = json!({ "op": 0, "t": kind, "s": self.next(), "d": d });
        self.socket.lock().unwrap().as_ref().expect("bot is connected").send(frame).unwrap();
    }

    /// Someone writes in a server channel or thread. Returns the message id.
    pub fn say(&self, user: &str, channel: &str, text: &str) -> String {
        let id = format!("{}", 5000 + self.next());
        let mentions: Vec<Value> = if text.contains(&format!("<@{DISCORD_BOT}>")) { vec![json!({ "id": DISCORD_BOT, "username": "hyperbeam" })] } else { vec![] };
        self.dispatch(
            "MESSAGE_CREATE",
            json!({
                "id": id, "channel_id": channel, "guild_id": "G1", "type": 0, "content": text,
                "author": { "id": user, "username": user.to_lowercase(), "global_name": format!("{user} Name") },
                "member": {}, "mentions": mentions,
            }),
        );
        id
    }

    /// Someone presses a button on one of the bot's messages.
    pub fn click(&self, user: &str, channel: &str, message: &str, custom_id: &str) -> String {
        let token = format!("itoken-{}", self.next());
        self.dispatch(
            "INTERACTION_CREATE",
            json!({
                "id": format!("i{}", self.next()), "token": token, "type": 3, "channel_id": channel, "guild_id": "G1",
                "member": { "user": { "id": user, "username": user.to_lowercase(), "global_name": format!("{user} Name") } },
                "message": { "id": message },
                "data": { "custom_id": custom_id, "component_type": 2 },
            }),
        );
        token
    }

    pub fn calls_to(&self, method: &str, path_contains: &str) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().iter().filter(|(m, p, _)| m == method && p.contains(path_contains)).map(|(_, p, body)| (p.clone(), body.clone())).collect()
    }

    /// The custom ids of the buttons on a message the bot sent.
    pub fn buttons_of(body: &Value) -> Vec<(String, String)> {
        body["components"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|row| row["components"].as_array().cloned().unwrap_or_default())
            .map(|b| (b["label"].as_str().unwrap_or_default().to_string(), b["custom_id"].as_str().unwrap_or_default().to_string()))
            .collect()
    }

    /// Wait for a call whose path contains `path_contains` and whose body satisfies `matches`.
    pub async fn wait_for(&self, method: &str, path_contains: &str, matches: impl Fn(&Value) -> bool) -> (String, Value) {
        eventually(&format!("a matching {method} …{path_contains} call"), || self.calls_to(method, path_contains).into_iter().find(|(_, body)| matches(body))).await
    }
}

async fn discord_api(State(discord): State<FakeDiscord>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some(&format!("Bot {DISCORD_TOKEN}")) {
        return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "message": "401: Unauthorized" }))).into_response();
    }
    let path = uri.path().trim_start_matches("/api").to_string();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let record = |id: Option<&Value>| {
        // Recorded with the id Discord gave the new message, as `_id`.
        let mut recorded = body.clone();
        if let (Some(id), Some(fields)) = (id, recorded.as_object_mut()) {
            fields.insert("_id".into(), id.clone());
        }
        discord.calls.lock().unwrap().push((method.to_string(), path.clone(), recorded));
    };
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let answer = match (method.as_str(), parts.as_slice()) {
        ("GET", ["gateway", "bot"]) => json!({ "url": format!("{}/gateway", discord.base.lock().unwrap().replace("http://", "ws://")) }),
        ("GET", ["users", "@me"]) => json!({ "id": DISCORD_BOT, "username": "hyperbeam" }),
        ("GET", ["users", "@me", "guilds"]) => json!([{ "id": "G1", "name": "Acme" }]),
        ("GET", ["guilds", _, "channels"]) => json!([
            { "id": "100", "name": "dev", "type": 0 },
            { "id": "101", "name": "voice", "type": 2 },
            { "id": "102", "name": "random", "type": 0 },
        ]),
        // Channels numbered from 7000 are threads of #dev.
        ("GET", ["channels", id]) => match id.parse::<u64>() {
            Ok(n) if n >= 7000 => json!({ "id": id, "type": 11, "parent_id": "100" }),
            _ => json!({ "id": id, "type": 0 }),
        },
        ("GET", ["channels", _, "messages"]) => json!([
            { "id": "2", "content": "It is the session cache, see https://thread.example/trace", "author": { "id": "8", "username": "sam", "global_name": "Sam" } },
            { "id": "1", "content": "Checkout is failing again", "author": { "id": "8", "username": "sam", "global_name": "Sam" } },
        ]),
        ("POST", ["channels", _, "messages", message, "threads"]) => json!({ "id": format!("9{message}"), "type": 11 }),
        ("POST", ["channels", id, "messages"]) if id.starts_with("dm-") && *discord.dms_closed.lock().unwrap() => {
            record(None);
            return (StatusCode::FORBIDDEN, axum::Json(json!({ "message": "Cannot send messages to this user", "code": 50007 }))).into_response();
        }
        ("POST", ["channels", _, "messages"]) => json!({ "id": format!("{}", 8000 + discord.next()) }),
        ("PATCH", ["channels", _, "messages", id]) => json!({ "id": id }),
        ("POST", ["users", "@me", "channels"]) => json!({ "id": format!("dm-{}", body["recipient_id"].as_str().unwrap_or_default()) }),
        ("POST", ["webhooks", ..]) => json!({ "id": format!("{}", 8000 + discord.next()) }),
        _ => {
            record(None);
            return StatusCode::NO_CONTENT.into_response();
        }
    };
    record(answer.get("id"));
    axum::Json(answer).into_response()
}

async fn discord_gateway(State(discord): State<FakeDiscord>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| async move {
        let identified = discord.identified.clone();
        pump(socket, discord.socket.clone(), discord.connections.clone(), json!({ "op": 10, "d": { "heartbeat_interval": 45_000 } }), move |frame| match frame["op"].as_u64() {
            Some(2) => {
                *identified.lock().unwrap() = Some(frame["d"].clone());
                Some(json!({ "op": 0, "t": "READY", "s": 1, "d": { "user": { "id": DISCORD_BOT, "username": "hyperbeam" }, "application": { "id": "APP" }, "session_id": "s1" } }))
            }
            Some(1) => Some(json!({ "op": 11 })),
            _ => None,
        })
        .await;
    })
}
