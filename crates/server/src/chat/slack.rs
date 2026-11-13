//! Slack, over Socket Mode: the server dials out to Slack and events,
//! button presses and acknowledgements all travel on that one WebSocket.
//! Calls the other way use the Web API with the bot token.

use super::{Author, BotInfo, Button, ChannelInfo, Click, Inbound, Incoming, Origin, Outgoing, Place, Platform, Private, StatusCell, Style, set_status, text};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Slack takes 12,000 characters of Markdown in a message.
const LIMIT: usize = 11_000;
/// Errors that will not go away by trying again.
const FATAL: [&str; 6] = ["invalid_auth", "not_authed", "account_inactive", "token_revoked", "not_allowed_token_type", "missing_scope"];

#[derive(Clone, Default)]
struct Me {
    user_id: String,
    team_id: String,
}

#[derive(Clone)]
struct Member {
    name: String,
    email: Option<String>,
}

enum Body<'a> {
    Json(Value),
    Form(&'a [(&'a str, String)]),
}

pub struct Slack {
    http: reqwest::Client,
    api: String,
    bot_token: String,
    app_token: String,
    me: RwLock<Me>,
    members: Mutex<HashMap<String, Member>>,
    /// Messages already handled: Slack sends again what it thinks was missed.
    seen: Mutex<VecDeque<String>>,
}

impl Slack {
    pub fn new(http: reqwest::Client, api: &str, bot_token: String, app_token: String) -> Self {
        Slack {
            http,
            api: api.trim_end_matches('/').to_string(),
            bot_token,
            app_token,
            me: RwLock::default(),
            members: Mutex::default(),
            seen: Mutex::default(),
        }
    }

    async fn call_as(&self, token: &str, method: &str, body: Body<'_>) -> Result<Value> {
        for attempt in 0.. {
            let request = self.http.post(format!("{}/{method}", self.api)).bearer_auth(token).timeout(Duration::from_secs(30));
            let request = match &body {
                Body::Json(value) => request.json(value),
                Body::Form(fields) => request.form(fields),
            };
            let response = request.send().await.with_context(|| format!("Slack {method}"))?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                let wait = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
                tokio::time::sleep(Duration::from_secs(wait.min(30))).await;
                continue;
            }
            let value: Value = response.json().await.with_context(|| format!("Slack {method}: unreadable answer"))?;
            if value["ok"] == true {
                return Ok(value);
            }
            bail!("Slack {method}: {}", value["error"].as_str().unwrap_or("unknown error"));
        }
        unreachable!()
    }

    async fn call(&self, method: &str, body: Body<'_>) -> Result<Value> {
        self.call_as(&self.bot_token, method, body).await
    }

    fn me(&self) -> Me {
        self.me.read().unwrap().clone()
    }

    async fn whoami(&self) -> Result<Value> {
        let auth = self.call("auth.test", Body::Form(&[])).await?;
        *self.me.write().unwrap() = Me {
            user_id: auth["user_id"].as_str().unwrap_or_default().to_string(),
            team_id: auth["team_id"].as_str().unwrap_or_default().to_string(),
        };
        Ok(auth)
    }

    /// A workspace member's name, and their address if Slack vouches for it:
    /// a confirmed address of a real member of this workspace.
    async fn member(&self, id: &str) -> Member {
        if let Some(member) = self.members.lock().unwrap().get(id) {
            return member.clone();
        }
        let member = match self.call("users.info", Body::Form(&[("user", id.to_string())])).await {
            Ok(info) => {
                let user = &info["user"];
                let name = [&user["profile"]["display_name"], &user["real_name"], &user["name"]]
                    .into_iter()
                    .filter_map(Value::as_str)
                    .find(|n| !n.trim().is_empty())
                    .unwrap_or(id)
                    .to_string();
                let ours = user["team_id"].as_str() == Some(self.me().team_id.as_str());
                let vouched = ours
                    && user["is_bot"] != true
                    && user["deleted"] != true
                    && user["is_stranger"] != true
                    && user["is_email_confirmed"] != false;
                let email = user["profile"]["email"].as_str().filter(|e| vouched && e.contains('@')).map(str::to_string);
                Member { name, email }
            }
            Err(err) => {
                tracing::warn!("{err:#}");
                // Not remembered: the next message tries again.
                return Member { name: id.to_string(), email: None };
            }
        };
        self.members.lock().unwrap().insert(id.to_string(), member.clone());
        member
    }

    async fn decode(&self, raw: &str) -> String {
        let bot = self.me().user_id;
        let mut names = HashMap::new();
        for id in text::slack_mentions(raw) {
            if id != bot {
                let name = self.member(&id).await.name;
                names.insert(id, name);
            }
        }
        text::slack_decode(raw, &bot, |id| names.get(id).cloned())
    }

    /// True the first time a key is seen.
    fn first(&self, key: String) -> bool {
        let mut seen = self.seen.lock().unwrap();
        if seen.contains(&key) {
            return false;
        }
        seen.push_back(key);
        if seen.len() > 500 {
            seen.pop_front();
        }
        true
    }

    async fn on_event(&self, payload: &Value) -> Option<Inbound> {
        let event = &payload["event"];
        if event["type"] != "message" {
            return None;
        }
        // Only what a person typed. Edits, joins and the like carry a
        // subtype; anything posted through an app, the bot's own messages
        // and an agent's Slack tools included, carries an app or bot id.
        if !matches!(event["subtype"].as_str(), None | Some("file_share") | Some("thread_broadcast")) {
            return None;
        }
        if event.get("bot_id").is_some_and(|v| !v.is_null()) || event.get("app_id").is_some_and(|v| !v.is_null()) {
            return None;
        }
        let me = self.me();
        let user = event["user"].as_str().filter(|u| *u != me.user_id)?;
        let (channel, ts, raw) = (event["channel"].as_str()?, event["ts"].as_str()?, event["text"].as_str().unwrap_or_default());
        let dm = event["channel_type"] == "im";
        let thread = event["thread_ts"].as_str().map(str::to_string);
        let addressed = dm || raw.contains(&format!("<@{}>", me.user_id)) || raw.contains(&format!("<@{}|", me.user_id));
        // Ordinary channel talk: not ours, and not worth a lookup.
        if !addressed && thread.is_none() {
            return None;
        }
        if !self.first(format!("{channel}:{ts}")) {
            return None;
        }
        Some(Inbound::Message(Incoming {
            author: Author { id: user.to_string(), name: self.member(user).await.name },
            channel: channel.to_string(),
            thread,
            message_id: ts.to_string(),
            text: self.decode(raw).await,
            addressed,
            dm,
            external: payload["is_ext_shared_channel"] == true,
        }))
    }

    fn on_interaction(&self, payload: &Value) -> Option<Inbound> {
        if payload["type"] != "block_actions" {
            return None;
        }
        let action = &payload["actions"][0];
        // A link button opens its page; there is nothing for us to do.
        if action.get("url").is_some_and(|v| !v.is_null()) {
            return None;
        }
        let id = action["value"].as_str().or(action["action_id"].as_str())?;
        let channel = payload["container"]["channel_id"].as_str().or(payload["channel"]["id"].as_str())?;
        let message = payload["container"]["message_ts"].as_str().or(payload["message"]["ts"].as_str())?;
        let thread = payload["container"]["thread_ts"].as_str().or(payload["message"]["thread_ts"].as_str()).unwrap_or(message);
        let user = payload["user"]["id"].as_str()?;
        let name = payload["user"]["name"].as_str().or(payload["user"]["username"].as_str()).unwrap_or(user);
        Some(Inbound::Click(Click {
            author: Author { id: user.to_string(), name: name.to_string() },
            place: Place { channel: channel.to_string(), thread: thread.to_string() },
            message_id: message.to_string(),
            action: id.to_string(),
            token: None,
        }))
    }

    /// One Socket Mode connection, until it drops.
    async fn connection(&self, inbound: &mpsc::UnboundedSender<Inbound>, status: &StatusCell) -> Result<()> {
        let auth = self.whoami().await?;
        let opened = self.call_as(&self.app_token, "apps.connections.open", Body::Form(&[])).await?;
        let url = opened["url"].as_str().ok_or_else(|| anyhow!("Slack did not say where to connect"))?;
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.context("could not open the Socket Mode connection")?;
        set_status(status, "connected", auth["team"].as_str().unwrap_or_default());
        while let Some(frame) = socket.next().await {
            let frame = match frame? {
                Message::Text(text) => text,
                Message::Close(_) => break,
                _ => continue,
            };
            let Ok(envelope) = serde_json::from_str::<Value>(frame.as_str()) else { continue };
            // Slack expects each envelope acknowledged within three seconds,
            // or it sends it again.
            if let Some(id) = envelope["envelope_id"].as_str() {
                socket.send(Message::Text(json!({ "envelope_id": id }).to_string().into())).await?;
            }
            let payload = &envelope["payload"];
            let event = match envelope["type"].as_str() {
                // Slack recycles connections now and then and says so first.
                Some("disconnect") => break,
                Some("events_api") => self.on_event(payload).await,
                Some("interactive") => self.on_interaction(payload),
                _ => None,
            };
            if let Some(event) = event
                && inbound.send(event).is_err()
            {
                break;
            }
        }
        Ok(())
    }

    fn blocks(message: &Outgoing, markdown: bool) -> Vec<Value> {
        let body = text::slack_defang(&message.text);
        let mut blocks = vec![if markdown {
            json!({ "type": "markdown", "text": body })
        } else {
            let mrkdwn: String = text::to_mrkdwn(&body).chars().take(3000).collect();
            json!({ "type": "section", "text": { "type": "mrkdwn", "text": mrkdwn } })
        }];
        // An actions block holds 25 elements.
        for (row, buttons) in message.buttons.chunks(25).enumerate() {
            let elements: Vec<Value> = buttons.iter().enumerate().map(|(i, button)| Self::button(button, row * 25 + i)).collect();
            blocks.push(json!({ "type": "actions", "elements": elements }));
        }
        blocks
    }

    fn button(button: &Button, index: usize) -> Value {
        let mut element = json!({
            "type": "button",
            "text": { "type": "plain_text", "text": button.label, "emoji": true },
            // Unique within the message; what was pressed comes back as the value.
            "action_id": format!("b{index}"),
        });
        match &button.url {
            Some(url) => element["url"] = json!(url),
            None => element["value"] = json!(button.id),
        }
        match button.style {
            Style::Primary => element["style"] = json!("primary"),
            Style::Danger => element["style"] = json!("danger"),
            Style::Plain => {}
        }
        element
    }

    /// Send a message with `chat.postMessage` or `chat.update`. Markdown
    /// goes as a Markdown block; a workspace that refuses that gets Slack's
    /// own markup instead.
    async fn send(&self, method: &str, mut body: Value, message: &Outgoing) -> Result<Value> {
        // Shown in notifications and by clients that do not render blocks.
        body["text"] = json!(text::to_mrkdwn(&text::slack_defang(&message.text)));
        body["blocks"] = json!(Self::blocks(message, true));
        match self.call(method, Body::Json(body.clone())).await {
            Err(err) if err.to_string().contains("invalid_blocks") => {
                body["blocks"] = json!(Self::blocks(message, false));
                self.call(method, Body::Json(body)).await
            }
            other => other,
        }
    }
}

#[async_trait]
impl Platform for Slack {
    async fn check(&self) -> Result<BotInfo> {
        let auth = self.whoami().await.context("the bot token was not accepted")?;
        if !self.app_token.starts_with("xapp-") {
            bail!("the app-level token starts with `xapp-`. Create one under Basic Information → App-Level Tokens, with the `connections:write` scope");
        }
        self.call_as(&self.app_token, "apps.connections.open", Body::Form(&[]))
            .await
            .context("the app-level token was not accepted. Is Socket Mode turned on for the app?")?;
        Ok(BotInfo {
            workspace: auth["team"].as_str().unwrap_or_default().to_string(),
            bot_name: auth["user"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn run(&self, inbound: mpsc::UnboundedSender<Inbound>, status: StatusCell) {
        let mut wait = Duration::from_secs(1);
        loop {
            let started = std::time::Instant::now();
            match self.connection(&inbound, &status).await {
                Ok(()) => {
                    wait = Duration::from_secs(1);
                    // Slack closed the connection, as it does from time to time.
                    tokio::time::sleep(wait).await;
                }
                Err(err) => {
                    let detail = format!("{err:#}");
                    tracing::warn!("Slack connection: {detail}");
                    if FATAL.iter().any(|code| detail.contains(code)) {
                        // Wrong until an admin changes the tokens, which restarts us.
                        wait = Duration::from_secs(60);
                    } else if started.elapsed() > Duration::from_secs(60) {
                        wait = Duration::from_secs(1);
                    }
                    set_status(&status, "error", detail);
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(60));
                    continue;
                }
            }
            if inbound.is_closed() {
                return;
            }
            set_status(&status, "connecting", "");
        }
    }

    fn limit(&self) -> usize {
        LIMIT
    }

    fn starts_in_dm(&self) -> bool {
        true
    }

    fn thread_key(&self, message: &Incoming) -> Place {
        Place { channel: message.channel.clone(), thread: message.thread.clone().unwrap_or_else(|| message.message_id.clone()) }
    }

    async fn origin(&self, message: &Incoming) -> Result<Origin> {
        Ok(Origin {
            channel: message.channel.clone(),
            dm: message.dm,
            external: message.external,
            thread: message.thread.as_ref().map(|thread| Place { channel: message.channel.clone(), thread: thread.clone() }),
        })
    }

    /// On Slack a thread is simply the replies to a message.
    async fn open_thread(&self, message: &Incoming, _title: &str) -> Result<Place> {
        Ok(Place { channel: message.channel.clone(), thread: message.message_id.clone() })
    }

    async fn post(&self, place: &Place, message: &Outgoing) -> Result<String> {
        let body = json!({ "channel": place.channel, "thread_ts": place.thread, "unfurl_links": false, "unfurl_media": false });
        let sent = self.send("chat.postMessage", body, message).await?;
        Ok(sent["ts"].as_str().unwrap_or_default().to_string())
    }

    async fn update(&self, place: &Place, message_id: &str, message: &Outgoing) -> Result<()> {
        self.send("chat.update", json!({ "channel": place.channel, "ts": message_id }), message).await?;
        Ok(())
    }

    async fn whisper(&self, to: &Private, text: &str) -> Result<()> {
        let mut body = json!({ "channel": to.channel, "text": text::to_mrkdwn(text) });
        if let Some(thread) = &to.thread {
            body["thread_ts"] = json!(thread);
        }
        // A direct message is already private.
        if to.channel.starts_with('D') {
            self.call("chat.postMessage", Body::Json(body)).await?;
        } else {
            body["user"] = json!(to.user);
            self.call("chat.postEphemeral", Body::Json(body)).await?;
        }
        Ok(())
    }

    async fn react(&self, channel: &str, message_id: &str) -> Result<()> {
        match self.call("reactions.add", Body::Json(json!({ "channel": channel, "timestamp": message_id, "name": "eyes" }))).await {
            Err(err) if !err.to_string().contains("already_reacted") => Err(err),
            _ => Ok(()),
        }
    }

    async fn history(&self, place: &Place, before: &str) -> Result<Vec<(String, String)>> {
        let replies = self
            .call("conversations.replies", Body::Form(&[("channel", place.channel.clone()), ("ts", place.thread.clone()), ("limit", "50".into())]))
            .await?;
        let me = self.me();
        let mut out = Vec::new();
        for message in replies["messages"].as_array().into_iter().flatten() {
            let user = message["user"].as_str();
            if message["ts"].as_str() == Some(before) || user == Some(me.user_id.as_str()) {
                continue;
            }
            // Alerts and other app posts often carry their words in an attachment.
            let raw = [&message["text"], &message["attachments"][0]["fallback"], &message["attachments"][0]["text"]]
                .into_iter()
                .filter_map(Value::as_str)
                .find(|t| !t.trim().is_empty())
                .unwrap_or_default();
            let author = match user {
                Some(id) if message.get("bot_id").is_none_or(Value::is_null) => self.member(id).await.name,
                _ => [&message["username"], &message["bot_profile"]["name"]].into_iter().filter_map(Value::as_str).next().unwrap_or("An app").to_string(),
            };
            out.push((author, self.decode(raw).await));
        }
        Ok(out)
    }

    async fn email(&self, user_id: &str) -> Result<Option<String>> {
        Ok(self.member(user_id).await.email)
    }

    async fn channels(&self) -> Result<Vec<ChannelInfo>> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..10 {
            let page = self
                .call(
                    "users.conversations",
                    Body::Form(&[
                        ("types", "public_channel,private_channel".into()),
                        ("exclude_archived", "true".into()),
                        ("limit", "200".into()),
                        ("cursor", cursor.clone()),
                    ]),
                )
                .await?;
            for channel in page["channels"].as_array().into_iter().flatten() {
                out.push(ChannelInfo {
                    id: channel["id"].as_str().unwrap_or_default().to_string(),
                    name: format!("#{}", channel["name"].as_str().unwrap_or_default()),
                    private: channel["is_private"] == true,
                    external: channel["is_ext_shared"] == true || channel["is_pending_ext_shared"] == true,
                });
            }
            cursor = page["response_metadata"]["next_cursor"].as_str().unwrap_or_default().to_string();
            if cursor.is_empty() {
                break;
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_become_blocks_with_buttons() {
        let message = Outgoing {
            text: "**Question:** Postgres or SQLite? <!channel>".into(),
            buttons: vec![Button::new("answer:q:0", "Postgres"), Button::new("stop", "Stop").styled(Style::Danger), Button::link("Watch", "https://hb.example/s/1")],
        };
        let blocks = Slack::blocks(&message, true);
        assert_eq!(blocks[0]["type"], "markdown");
        assert!(!blocks[0]["text"].as_str().unwrap().contains("<!channel>"));
        let elements = blocks[1]["elements"].as_array().unwrap();
        assert_eq!(elements[0]["value"], "answer:q:0");
        assert_eq!(elements[1]["style"], "danger");
        assert_eq!(elements[2]["url"], "https://hb.example/s/1");
        assert!(elements[2].get("value").is_none());
        let ids: std::collections::HashSet<&str> = elements.iter().map(|e| e["action_id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 3);
        // The fallback for a workspace without Markdown blocks.
        assert_eq!(Slack::blocks(&message, false)[0]["text"]["text"], "*Question:* Postgres or SQLite? <\u{200b}!channel>");
    }
}
