//! Discord, over the Gateway: the server dials out, identifies as the bot
//! and is sent messages and button presses on that WebSocket. Calls the
//! other way use the REST API with the bot token.

use super::{Author, BotInfo, Button, ChannelInfo, Click, Inbound, Incoming, Origin, Outgoing, Place, Platform, Private, StatusCell, Style, set_status};
use crate::sessions::clip;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use reqwest::Method;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Discord takes 2,000 characters in a message.
const LIMIT: usize = 1900;
/// Guilds, their messages, direct messages, and what messages say.
const INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15);
const EYES: &str = "%F0%9F%91%80";

#[derive(Clone, Default)]
struct Me {
    user_id: String,
    app_id: String,
}

#[derive(Clone)]
struct Chan {
    kind: u64,
    parent: Option<String>,
}

pub struct Discord {
    http: reqwest::Client,
    api: String,
    token: String,
    me: RwLock<Me>,
    channels: Mutex<HashMap<String, Chan>>,
}

/// A close code that reconnecting will not cure, in words an admin can act on.
fn fatal(code: u16) -> Option<&'static str> {
    match code {
        4004 => Some("Discord did not accept the bot token"),
        4013 | 4014 => Some(
            "Discord refused the connection because the bot may not read message content. \
             Turn on Message Content Intent under Bot → Privileged Gateway Intents in the Discord developer portal",
        ),
        _ => None,
    }
}

impl Discord {
    pub fn new(http: reqwest::Client, api: &str, token: String) -> Self {
        Discord { http, api: api.trim_end_matches('/').to_string(), token, me: RwLock::default(), channels: Mutex::default() }
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        for attempt in 0.. {
            let mut request = self
                .http
                .request(method.clone(), format!("{}{path}", self.api))
                .header("authorization", format!("Bot {}", self.token))
                .timeout(Duration::from_secs(30));
            if let Some(body) = &body {
                request = request.json(body);
            } else if method != Method::GET {
                request = request.header("content-length", "0");
            }
            let response = request.send().await.with_context(|| format!("Discord {method} {path}"))?;
            let status = response.status();
            let value: Value = response.json().await.unwrap_or(Value::Null);
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                let wait = value["retry_after"].as_f64().unwrap_or(1.0).clamp(0.1, 30.0);
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                continue;
            }
            if status.is_success() {
                return Ok(value);
            }
            bail!("Discord {method} {path}: {} ({status})", value["message"].as_str().unwrap_or("request failed"));
        }
        unreachable!()
    }

    fn me(&self) -> Me {
        self.me.read().unwrap().clone()
    }

    async fn channel(&self, id: &str) -> Result<Chan> {
        if let Some(chan) = self.channels.lock().unwrap().get(id) {
            return Ok(chan.clone());
        }
        let info = self.call(Method::GET, &format!("/channels/{id}"), None).await?;
        let chan = Chan { kind: info["type"].as_u64().unwrap_or(0), parent: info["parent_id"].as_str().map(str::to_string) };
        self.channels.lock().unwrap().insert(id.to_string(), chan.clone());
        Ok(chan)
    }

    fn author(user: &Value, member: &Value) -> Option<Author> {
        let id = user["id"].as_str()?;
        let name = [&member["nick"], &user["global_name"], &user["username"]].into_iter().filter_map(Value::as_str).find(|n| !n.is_empty()).unwrap_or(id);
        Some(Author { id: id.to_string(), name: name.to_string() })
    }

    fn on_message(&self, d: &Value) -> Option<Inbound> {
        let me = self.me();
        // Only what a person wrote: ordinary messages and replies.
        if d["author"]["bot"] == true || d["author"]["id"].as_str() == Some(me.user_id.as_str()) || !matches!(d["type"].as_u64(), Some(0) | Some(19)) {
            return None;
        }
        let mentioned: Vec<(&str, &str)> = d["mentions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|user| Some((user["id"].as_str()?, user["global_name"].as_str().or(user["username"].as_str())?)))
            .collect();
        let dm = d.get("guild_id").is_none_or(Value::is_null);
        let addressed = dm || mentioned.iter().any(|(id, _)| *id == me.user_id);
        let mut text = d["content"].as_str().unwrap_or_default().to_string();
        for (id, name) in &mentioned {
            let shown = if *id == me.user_id { String::new() } else { format!("@{name}") };
            text = text.replace(&format!("<@{id}>"), &shown).replace(&format!("<@!{id}>"), &shown);
        }
        Some(Inbound::Message(Incoming {
            author: Self::author(&d["author"], &d["member"])?,
            channel: d["channel_id"].as_str()?.to_string(),
            thread: None,
            message_id: d["id"].as_str()?.to_string(),
            text: text.trim().to_string(),
            addressed,
            dm,
            external: false,
        }))
    }

    /// A button press. Discord wants it acknowledged within three seconds,
    /// so that happens here; what it leads to follows in its own time.
    async fn on_interaction(&self, d: &Value) -> Option<Inbound> {
        if d["type"].as_u64() != Some(3) {
            return None;
        }
        let (id, token) = (d["id"].as_str()?, d["token"].as_str()?);
        if let Err(err) = self.call(Method::POST, &format!("/interactions/{id}/{token}/callback"), Some(json!({ "type": 6 }))).await {
            tracing::warn!("{err:#}");
        }
        let user = if d["member"]["user"].is_object() { &d["member"]["user"] } else { &d["user"] };
        Some(Inbound::Click(Click {
            author: Self::author(user, &d["member"])?,
            place: Place { channel: d["channel_id"].as_str()?.to_string(), thread: String::new() },
            message_id: d["message"]["id"].as_str()?.to_string(),
            action: d["data"]["custom_id"].as_str()?.to_string(),
            token: Some(token.to_string()),
        }))
    }

    /// One Gateway connection, until it drops.
    async fn connection(&self, inbound: &mpsc::UnboundedSender<Inbound>, status: &StatusCell) -> Result<()> {
        let gateway = self.call(Method::GET, "/gateway/bot", None).await?;
        let mut url = url::Url::parse(gateway["url"].as_str().ok_or_else(|| anyhow!("Discord did not say where to connect"))?)?;
        url.set_query(Some("v=10&encoding=json"));
        let (mut socket, _) = tokio_tungstenite::connect_async(url.as_str()).await.context("could not open the Gateway connection")?;

        let hello = loop {
            match socket.next().await.ok_or_else(|| anyhow!("the Gateway closed before saying hello"))?? {
                Message::Text(text) => break serde_json::from_str::<Value>(text.as_str())?,
                Message::Close(_) => bail!("the Gateway closed before saying hello"),
                _ => continue,
            }
        };
        let interval = Duration::from_millis(hello["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250));
        let identify = json!({
            "op": 2,
            "d": {
                "token": self.token,
                "intents": INTENTS,
                "properties": { "os": std::env::consts::OS, "browser": "hyperbeam", "device": "hyperbeam" },
            },
        });
        socket.send(Message::Text(identify.to_string().into())).await?;

        let mut beat = tokio::time::interval_at(tokio::time::Instant::now() + interval / 2, interval);
        let mut seq: Option<u64> = None;
        loop {
            tokio::select! {
                _ = beat.tick() => socket.send(Message::Text(json!({ "op": 1, "d": seq }).to_string().into())).await?,
                frame = socket.next() => {
                    let text = match frame {
                        None => return Ok(()),
                        Some(Err(err)) => return Err(err.into()),
                        Some(Ok(Message::Text(text))) => text,
                        Some(Ok(Message::Close(close))) => {
                            let code = close.map(|c| u16::from(c.code)).unwrap_or(1000);
                            match fatal(code) {
                                Some(why) => bail!("{why}"),
                                None => return Ok(()),
                            }
                        }
                        Some(Ok(_)) => continue,
                    };
                    let Ok(frame) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                    if let Some(s) = frame["s"].as_u64() {
                        seq = Some(s);
                    }
                    match frame["op"].as_u64() {
                        Some(0) => {
                            let d = &frame["d"];
                            let event = match frame["t"].as_str() {
                                Some("READY") => {
                                    *self.me.write().unwrap() = Me {
                                        user_id: d["user"]["id"].as_str().unwrap_or_default().to_string(),
                                        app_id: d["application"]["id"].as_str().unwrap_or_default().to_string(),
                                    };
                                    set_status(status, "connected", d["user"]["username"].as_str().unwrap_or_default());
                                    None
                                }
                                Some("MESSAGE_CREATE") => self.on_message(d),
                                Some("INTERACTION_CREATE") => self.on_interaction(d).await,
                                _ => None,
                            };
                            if let Some(event) = event && inbound.send(event).is_err() {
                                return Ok(());
                            }
                        }
                        // Asked for a heartbeat now.
                        Some(1) => socket.send(Message::Text(json!({ "op": 1, "d": seq }).to_string().into())).await?,
                        // Told to reconnect, or that the session is no good.
                        Some(7) | Some(9) => return Ok(()),
                        _ => {}
                    }
                }
            }
        }
    }

    fn body(message: &Outgoing) -> Value {
        // Rows of five buttons, five rows to a message.
        let rows: Vec<Value> = message
            .buttons
            .chunks(5)
            .take(5)
            .map(|row| json!({ "type": 1, "components": row.iter().map(Self::button).collect::<Vec<_>>() }))
            .collect();
        json!({
            "content": message.text.chars().take(2000).collect::<String>(),
            "components": rows,
            // People may be mentioned; @everyone and roles may not.
            "allowed_mentions": { "parse": ["users"] },
        })
    }

    fn button(button: &Button) -> Value {
        let label = clip(&button.label, 80);
        match &button.url {
            Some(url) => json!({ "type": 2, "style": 5, "label": label, "url": url }),
            None => {
                let style = match button.style {
                    Style::Primary => 1,
                    Style::Plain => 2,
                    Style::Danger => 4,
                };
                json!({ "type": 2, "style": style, "label": label, "custom_id": button.id })
            }
        }
    }
}

#[async_trait]
impl Platform for Discord {
    async fn check(&self) -> Result<BotInfo> {
        let me = self.call(Method::GET, "/users/@me", None).await.context("the bot token was not accepted")?;
        let guilds = self.call(Method::GET, "/users/@me/guilds", None).await?;
        let names: Vec<&str> = guilds.as_array().into_iter().flatten().filter_map(|g| g["name"].as_str()).collect();
        Ok(BotInfo { workspace: names.join(", "), bot_name: me["username"].as_str().unwrap_or_default().to_string() })
    }

    async fn run(&self, inbound: mpsc::UnboundedSender<Inbound>, status: StatusCell) {
        let mut wait = Duration::from_secs(1);
        loop {
            let started = std::time::Instant::now();
            match self.connection(&inbound, &status).await {
                Ok(()) => {
                    if inbound.is_closed() {
                        return;
                    }
                    set_status(&status, "connecting", "");
                    wait = Duration::from_secs(1);
                }
                Err(err) => {
                    let detail = format!("{err:#}");
                    tracing::warn!("Discord connection: {detail}");
                    if detail.contains("Discord refused") || detail.contains("did not accept") || detail.contains("401") {
                        // Wrong until an admin fixes it, which restarts us.
                        wait = Duration::from_secs(60);
                    } else if started.elapsed() > Duration::from_secs(60) {
                        wait = Duration::from_secs(1);
                    }
                    set_status(&status, "error", detail);
                }
            }
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    fn limit(&self) -> usize {
        LIMIT
    }

    /// A direct message has no threads to keep agents apart.
    fn starts_in_dm(&self) -> bool {
        false
    }

    fn thread_key(&self, message: &Incoming) -> Place {
        Place { channel: message.channel.clone(), thread: String::new() }
    }

    async fn origin(&self, message: &Incoming) -> Result<Origin> {
        if message.dm {
            return Ok(Origin { channel: message.channel.clone(), dm: true, external: false, thread: None });
        }
        let chan = self.channel(&message.channel).await?;
        Ok(match (chan.kind, chan.parent) {
            // Announcement, public and private threads.
            (10..=12, Some(parent)) => {
                Origin { channel: parent, dm: false, external: false, thread: Some(Place { channel: message.channel.clone(), thread: String::new() }) }
            }
            (1 | 3, _) => Origin { channel: message.channel.clone(), dm: true, external: false, thread: None },
            _ => Origin { channel: message.channel.clone(), dm: false, external: false, thread: None },
        })
    }

    async fn open_thread(&self, message: &Incoming, title: &str) -> Result<Place> {
        let thread = self
            .call(
                Method::POST,
                &format!("/channels/{}/messages/{}/threads", message.channel, message.message_id),
                Some(json!({ "name": clip(title, 90), "auto_archive_duration": 1440 })),
            )
            .await?;
        let id = thread["id"].as_str().ok_or_else(|| anyhow!("Discord did not return the new thread"))?;
        Ok(Place { channel: id.to_string(), thread: String::new() })
    }

    async fn post(&self, place: &Place, message: &Outgoing) -> Result<String> {
        let sent = self.call(Method::POST, &format!("/channels/{}/messages", place.channel), Some(Self::body(message))).await?;
        Ok(sent["id"].as_str().unwrap_or_default().to_string())
    }

    async fn update(&self, place: &Place, message_id: &str, message: &Outgoing) -> Result<()> {
        self.call(Method::PATCH, &format!("/channels/{}/messages/{message_id}", place.channel), Some(Self::body(message))).await?;
        Ok(())
    }

    async fn whisper(&self, to: &Private, text: &str) -> Result<()> {
        let content: String = text.chars().take(2000).collect();
        // After a button press: a reply only the person who pressed it sees.
        if let Some(token) = &to.token {
            let app_id = self.me().app_id;
            self.call(Method::POST, &format!("/webhooks/{app_id}/{token}"), Some(json!({ "content": content, "flags": 64 }))).await?;
            return Ok(());
        }
        // Otherwise a direct message: Discord has nothing more private.
        let direct = async {
            let dm = self.call(Method::POST, "/users/@me/channels", Some(json!({ "recipient_id": to.user }))).await?;
            let id = dm["id"].as_str().ok_or_else(|| anyhow!("Discord did not open a direct message"))?;
            self.call(Method::POST, &format!("/channels/{id}/messages"), Some(json!({ "content": content }))).await
        };
        if direct.await.is_err() {
            let notice = format!(
                "<@{}> I have something to tell you but couldn't send you a direct message. Allow direct messages from members of this server, then ask me again.",
                to.user
            );
            self.call(
                Method::POST,
                &format!("/channels/{}/messages", to.channel),
                Some(json!({ "content": notice, "allowed_mentions": { "users": [to.user] } })),
            )
            .await?;
        }
        Ok(())
    }

    async fn direct(&self, user_id: &str, message: &Outgoing) -> Result<()> {
        let dm = self.call(Method::POST, "/users/@me/channels", Some(json!({ "recipient_id": user_id }))).await?;
        let id = dm["id"].as_str().ok_or_else(|| anyhow!("Discord did not open a direct message"))?;
        self.call(Method::POST, &format!("/channels/{id}/messages"), Some(Self::body(message))).await?;
        Ok(())
    }

    async fn react(&self, channel: &str, message_id: &str) -> Result<()> {
        self.call(Method::PUT, &format!("/channels/{channel}/messages/{message_id}/reactions/{EYES}/@me"), None).await?;
        Ok(())
    }

    async fn history(&self, place: &Place, before: &str) -> Result<Vec<(String, String)>> {
        let messages = self.call(Method::GET, &format!("/channels/{}/messages?limit=50", place.channel), None).await?;
        let me = self.me();
        // Newest first from Discord; oldest first for the reader.
        Ok(messages
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .filter(|m| m["id"].as_str() != Some(before) && m["author"]["id"].as_str() != Some(me.user_id.as_str()))
            .filter_map(|m| {
                let author = Self::author(&m["author"], &m["member"])?;
                // Alerts often carry their words in an embed.
                let text = [&m["content"], &m["embeds"][0]["description"], &m["embeds"][0]["title"]]
                    .into_iter()
                    .filter_map(Value::as_str)
                    .find(|t| !t.trim().is_empty())?;
                Some((author.name, text.to_string()))
            })
            .collect())
    }

    /// Discord does not tell bots anyone's address.
    async fn email(&self, _user_id: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn channels(&self) -> Result<Vec<ChannelInfo>> {
        let guilds = self.call(Method::GET, "/users/@me/guilds", None).await?;
        let guilds: Vec<&Value> = guilds.as_array().into_iter().flatten().take(10).collect();
        let mut out = Vec::new();
        for guild in &guilds {
            let Some(id) = guild["id"].as_str() else { continue };
            let channels = self.call(Method::GET, &format!("/guilds/{id}/channels"), None).await?;
            for channel in channels.as_array().into_iter().flatten() {
                // Text and announcement channels: the ones a thread can start in.
                if !matches!(channel["type"].as_u64(), Some(0) | Some(5)) {
                    continue;
                }
                let name = channel["name"].as_str().unwrap_or_default();
                out.push(ChannelInfo {
                    id: channel["id"].as_str().unwrap_or_default().to_string(),
                    name: if guilds.len() > 1 { format!("{} / #{name}", guild["name"].as_str().unwrap_or_default()) } else { format!("#{name}") },
                    private: false,
                    external: false,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot() -> Discord {
        let discord = Discord::new(reqwest::Client::new(), "http://unused", "token".into());
        *discord.me.write().unwrap() = Me { user_id: "999".into(), app_id: "app".into() };
        discord
    }

    #[test]
    fn a_mention_is_addressed_and_stripped() {
        let d = json!({
            "id": "m1", "channel_id": "c1", "guild_id": "g1", "type": 0,
            "content": "<@999> ask <@5> about acme/web",
            "author": { "id": "7", "username": "mia", "global_name": "Mia" },
            "member": { "nick": "Mia M" },
            "mentions": [{ "id": "999", "username": "hyperbeam" }, { "id": "5", "username": "sam" }],
        });
        let Some(Inbound::Message(message)) = bot().on_message(&d) else { panic!("not a message") };
        assert!(message.addressed && !message.dm);
        assert_eq!(message.text, "ask @sam about acme/web");
        assert_eq!(message.author.name, "Mia M");
    }

    #[test]
    fn bots_and_system_messages_are_ignored() {
        let base = json!({ "id": "m1", "channel_id": "c1", "guild_id": "g1", "type": 0, "content": "hi", "author": { "id": "7", "username": "mia" }, "mentions": [] });
        let Some(Inbound::Message(message)) = bot().on_message(&base) else { panic!("not a message") };
        assert!(!message.addressed);
        let mut from_bot = base.clone();
        from_bot["author"]["bot"] = json!(true);
        assert!(bot().on_message(&from_bot).is_none());
        let mut joined = base.clone();
        joined["type"] = json!(7);
        assert!(bot().on_message(&joined).is_none());
    }

    #[test]
    fn buttons_fill_rows_of_five() {
        let message = Outgoing {
            text: "x".repeat(2500),
            buttons: (0..7).map(|i| Button::new(format!("repo:{i}"), format!("repo {i}"))).chain([Button::link("Watch", "https://hb.example")]).collect(),
        };
        let body = Discord::body(&message);
        assert_eq!(body["content"].as_str().unwrap().len(), 2000);
        assert_eq!(body["components"][0]["components"].as_array().unwrap().len(), 5);
        assert_eq!(body["components"][1]["components"][2]["style"], 5);
        assert_eq!(body["components"][1]["components"][2]["url"], "https://hb.example");
        assert_eq!(body["components"][0]["components"][0]["custom_id"], "repo:0");
        assert_eq!(body["allowed_mentions"]["parse"], json!(["users"]));
    }

    #[test]
    fn close_codes_an_admin_must_fix() {
        assert!(fatal(4014).unwrap().contains("Message Content Intent"));
        assert!(fatal(4004).is_some());
        assert!(fatal(1001).is_none());
    }
}
