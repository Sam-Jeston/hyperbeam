//! The Slack and Discord clients against stand-ins for both platforms:
//! the connection each bot dials out on, what it makes of what people do,
//! and the calls it sends back. Needs no database.

#[path = "support/chat.rs"]
mod fakes;

use fakes::{DISCORD_BOT, DISCORD_TOKEN, FakeDiscord, FakeSlack, SLACK_APP_TOKEN, SLACK_BOT, SLACK_BOT_TOKEN, eventually};
use hyperbeam_server::chat::discord::Discord;
use hyperbeam_server::chat::slack::Slack;
use hyperbeam_server::chat::{Button, Inbound, Outgoing, Place, Platform, Private, Status, StatusCell, Style};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

fn connecting() -> StatusCell {
    Arc::new(Mutex::new(Status { state: "connecting", detail: String::new() }))
}

async fn next(rx: &mut mpsc::UnboundedReceiver<Inbound>) -> Inbound {
    tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("the bot passed something on").expect("the bot is running")
}

async fn connected(status: &StatusCell) {
    eventually("the bot to connect", || (status.lock().unwrap().state == "connected").then_some(())).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn slack_speaks_socket_mode() {
    let fake = FakeSlack::default();
    let api = fake.start().await;
    let slack = Arc::new(Slack::new(reqwest::Client::new(), &api, SLACK_BOT_TOKEN.into(), SLACK_APP_TOKEN.into()));

    // Tokens are checked before they are stored.
    let info = slack.check().await.unwrap();
    assert_eq!((info.workspace.as_str(), info.bot_name.as_str()), ("Acme", "hyperbeam"));
    let wrong = Slack::new(reqwest::Client::new(), &api, "xoxb-wrong".into(), SLACK_APP_TOKEN.into());
    assert!(format!("{:#}", wrong.check().await.unwrap_err()).contains("bot token"));
    let wrong = Slack::new(reqwest::Client::new(), &api, SLACK_BOT_TOKEN.into(), "xoxb-not-an-app-token".into());
    assert!(format!("{:#}", wrong.check().await.unwrap_err()).contains("xapp-"));

    let (tx, mut rx) = mpsc::unbounded_channel();
    let status = connecting();
    tokio::spawn({
        let (slack, status) = (slack.clone(), status.clone());
        async move { slack.run(tx, status).await }
    });
    connected(&status).await;
    fake.connected().await;
    assert_eq!(status.lock().unwrap().detail, "Acme");

    // Channel talk that is not for the bot is dropped without a lookup; so
    // is anything posted through an app, the bot's own messages included.
    fake.say("UMIA", "CDEV", None, "morning all");
    fake.say_with("UMIA", "CDEV", None, &format!("<@{SLACK_BOT}> posted by a tool"), json!({ "app_id": "A1" }));
    fake.say_with(SLACK_BOT, "CDEV", Some("1.0"), "my own reply", json!({ "bot_id": "B1" }));
    fake.say_with("UMIA", "CDEV", Some("1.0"), "edited", json!({ "subtype": "message_changed" }));

    // A mention is addressed to the bot, with Slack's markup undone.
    let ts = fake.say("UMIA", "CDEV", None, &format!("<@{SLACK_BOT}> fix <https://ci.example/run/1|the build> &amp; tell <@UOLIVE>"));
    let Inbound::Message(message) = next(&mut rx).await else { panic!("expected a message") };
    assert_eq!(message.text, "fix https://ci.example/run/1 & tell @Olive Owner");
    assert_eq!((message.author.id.as_str(), message.author.name.as_str()), ("UMIA", "Mia Member"));
    assert!(message.addressed && !message.dm && !message.external && message.thread.is_none());
    assert_eq!(message.message_id, ts);
    // The thread it would start is the replies to that message.
    assert_eq!(slack.thread_key(&message), Place { channel: "CDEV".into(), thread: ts.clone() });
    assert_eq!(slack.open_thread(&message, "Fix the build").await.unwrap(), Place { channel: "CDEV".into(), thread: ts.clone() });
    assert!(slack.origin(&message).await.unwrap().thread.is_none());

    // A reply in a thread is passed on whether or not it mentions the bot.
    fake.say_with("UOLIVE", "CDEV", Some(&ts), "also bump the cache", json!({ "is_ext_shared_channel": true }));
    let Inbound::Message(reply) = next(&mut rx).await else { panic!("expected a message") };
    assert!(!reply.addressed && reply.external);
    assert_eq!(slack.thread_key(&reply), Place { channel: "CDEV".into(), thread: ts.clone() });
    assert_eq!(slack.origin(&reply).await.unwrap().thread, Some(Place { channel: "CDEV".into(), thread: ts.clone() }));

    // A direct message needs no mention.
    fake.say("UMIA", "DMIA", None, "what is running?");
    let Inbound::Message(direct) = next(&mut rx).await else { panic!("expected a message") };
    assert!(direct.addressed && direct.dm);

    // A button press arrives with where it was pressed.
    fake.click("UMIA", "CDEV", "1700000001.000042", &ts, "answer:q1:0");
    let Inbound::Click(click) = next(&mut rx).await else { panic!("expected a click") };
    assert_eq!(click.action, "answer:q1:0");
    assert_eq!(click.place, Place { channel: "CDEV".into(), thread: ts.clone() });
    assert_eq!(click.message_id, "1700000001.000042");

    // Slack sends again what goes unacknowledged; every envelope was.
    eventually("every envelope to be acknowledged", || (fake.acks.lock().unwrap().len() == 8).then_some(())).await;

    // Outbound: a message in the thread, as Markdown with buttons.
    let place = Place { channel: "CDEV".into(), thread: ts.clone() };
    let message = Outgoing { text: "**Question:** ship it?".into(), buttons: vec![Button::new("answer:q1:0", "Yes"), Button::new("stop", "Stop").styled(Style::Danger)] };
    let id = slack.post(&place, &message).await.unwrap();
    let posted = fake.calls_to("chat.postMessage").pop().unwrap();
    assert_eq!((posted["channel"].as_str(), posted["thread_ts"].as_str()), (Some("CDEV"), Some(ts.as_str())));
    assert_eq!(FakeSlack::text_of(&posted), "**Question:** ship it?");
    assert_eq!(posted["text"], "*Question:* ship it?");
    assert_eq!(FakeSlack::buttons_of(&posted), vec![("Yes".to_string(), "answer:q1:0".to_string()), ("Stop".to_string(), "stop".to_string())]);
    slack.update(&place, &id, &Outgoing::text("Answered.")).await.unwrap();
    assert_eq!(fake.calls_to("chat.update").pop().unwrap()["ts"], id.as_str());

    // A private note is ephemeral in a channel, and an ordinary message in a DM.
    slack.whisper(&Private { user: "UMIA".into(), channel: "CDEV".into(), thread: Some(ts.clone()), token: None }, "Link your account").await.unwrap();
    let note = fake.calls_to("chat.postEphemeral").pop().unwrap();
    assert_eq!((note["user"].as_str(), note["thread_ts"].as_str()), (Some("UMIA"), Some(ts.as_str())));
    slack.whisper(&Private { user: "UMIA".into(), channel: "DMIA".into(), thread: None, token: None }, "Link your account").await.unwrap();
    assert_eq!(fake.calls_to("chat.postMessage").pop().unwrap()["channel"], "DMIA");
    // A notification goes to the member directly, link and all.
    let notice = Outgoing { text: "**“Fix checkout” failed**".into(), buttons: vec![Button::link("Open", "https://hb.example/s/1")] };
    slack.direct("UMIA", &notice).await.unwrap();
    let sent = fake.calls_to("chat.postMessage").pop().unwrap();
    assert_eq!(sent["channel"], "UMIA");
    assert!(sent.get("thread_ts").is_none() && sent.to_string().contains("https://hb.example/s/1"), "{sent}");
    slack.react("CDEV", &ts).await.unwrap();
    assert_eq!(fake.calls_to("reactions.add").pop().unwrap()["timestamp"], ts.as_str());

    // An address counts only for a confirmed member of this workspace.
    assert_eq!(slack.email("UMIA").await.unwrap().as_deref(), Some("mia@example.com"));
    assert_eq!(slack.email("UGUEST").await.unwrap(), None);

    // The thread so far, without the request itself or the bot's own words.
    *fake.replies.lock().unwrap() = vec![
        json!({ "ts": "1.0", "user": "UOLIVE", "text": "Checkout is failing again" }),
        json!({ "ts": "1.1", "bot_id": "B9", "username": "Sentry", "text": "", "attachments": [{ "fallback": "TypeError in cart.ts" }] }),
        json!({ "ts": "1.2", "user": SLACK_BOT, "text": "Working…" }),
        json!({ "ts": "1.3", "user": "UMIA", "text": format!("<@{SLACK_BOT}> fix it") }),
    ];
    let history = slack.history(&Place { channel: "CDEV".into(), thread: "1.0".into() }, "1.3").await.unwrap();
    assert_eq!(history, vec![("Olive Owner".to_string(), "Checkout is failing again".to_string()), ("Sentry".to_string(), "TypeError in cart.ts".to_string())]);

    let channels = slack.channels().await.unwrap();
    assert_eq!(channels.iter().map(|c| (c.name.as_str(), c.external)).collect::<Vec<_>>(), vec![("#dev", false), ("#partners", true)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn discord_speaks_the_gateway() {
    let fake = FakeDiscord::default();
    let api = fake.start().await;
    let discord = Arc::new(Discord::new(reqwest::Client::new(), &api, DISCORD_TOKEN.into()));

    let info = discord.check().await.unwrap();
    assert_eq!((info.workspace.as_str(), info.bot_name.as_str()), ("Acme", "hyperbeam"));
    let wrong = Discord::new(reqwest::Client::new(), &api, "wrong".into());
    assert!(format!("{:#}", wrong.check().await.unwrap_err()).contains("bot token"));

    let (tx, mut rx) = mpsc::unbounded_channel();
    let status = connecting();
    tokio::spawn({
        let (discord, status) = (discord.clone(), status.clone());
        async move { discord.run(tx, status).await }
    });
    connected(&status).await;
    // It asked to read what messages say, which replies in a thread need.
    let identified = fake.identified.lock().unwrap().clone().unwrap();
    assert_eq!(identified["token"], DISCORD_TOKEN);
    assert_ne!(identified["intents"].as_u64().unwrap() & (1 << 15), 0);

    // A mention in a channel: the bot opens a thread on it.
    let id = fake.say("7", "100", &format!("<@{DISCORD_BOT}> fix the build"));
    let Inbound::Message(message) = next(&mut rx).await else { panic!("expected a message") };
    assert_eq!(message.text, "fix the build");
    assert!(message.addressed && !message.dm);
    let origin = discord.origin(&message).await.unwrap();
    assert_eq!(origin.channel, "100");
    assert!(origin.thread.is_none());
    let place = discord.open_thread(&message, "Fix the build").await.unwrap();
    assert_eq!(place, Place { channel: format!("9{id}"), thread: String::new() });
    assert_eq!(fake.calls_to("POST", "/threads").pop().unwrap().1["name"], "Fix the build");

    // A message inside a thread: enabled or not is its parent's business.
    let reply = fake.say("7", &place.channel, "and bump the cache");
    let Inbound::Message(inside) = next(&mut rx).await else { panic!("expected a message") };
    assert!(!inside.addressed);
    assert_eq!(discord.thread_key(&inside), place);
    let origin = discord.origin(&inside).await.unwrap();
    assert_eq!((origin.channel.as_str(), origin.thread), ("100", Some(place.clone())));

    // Outbound: Markdown and rows of buttons, with @everyone disarmed.
    let message = Outgoing { text: "**Question:** ship it?".into(), buttons: vec![Button::new("answer:q1:0", "Yes"), Button::link("Watch", "https://hb.example/s/1")] };
    let sent = discord.post(&place, &message).await.unwrap();
    let (path, body) = fake.calls_to("POST", &format!("/channels/{}/messages", place.channel)).pop().unwrap();
    assert!(path.ends_with("/messages"));
    assert_eq!(body["content"], "**Question:** ship it?");
    assert_eq!(body["allowed_mentions"]["parse"], json!(["users"]));
    assert_eq!(FakeDiscord::buttons_of(&body)[0], ("Yes".to_string(), "answer:q1:0".to_string()));
    discord.update(&place, &sent, &Outgoing::text("Answered.")).await.unwrap();
    assert_eq!(fake.calls_to("PATCH", &sent).len(), 1);
    discord.react("100", &reply).await.unwrap();
    assert_eq!(fake.calls_to("PUT", "/reactions/").len(), 1);

    // A press is acknowledged at once, and can then be answered privately.
    let token = fake.click("7", &place.channel, &sent, "answer:q1:0");
    let Inbound::Click(click) = next(&mut rx).await else { panic!("expected a click") };
    assert_eq!((click.action.as_str(), click.place.clone(), click.message_id.as_str()), ("answer:q1:0", place.clone(), sent.as_str()));
    assert_eq!(fake.calls_to("POST", "/callback").pop().unwrap().1["type"], 6);
    let to = Private { user: "7".into(), channel: place.channel.clone(), thread: None, token: Some(token.clone()) };
    discord.whisper(&to, "Already answered.").await.unwrap();
    let (path, body) = fake.calls_to("POST", "/webhooks/").pop().unwrap();
    assert_eq!(path, format!("/webhooks/APP/{token}"));
    assert_eq!(body["flags"], 64);

    // Without a press to answer, a private note is a direct message; if
    // those are closed, the person is told so where they asked.
    let to = Private { user: "7".into(), channel: "100".into(), thread: None, token: None };
    discord.whisper(&to, "Link your account: https://hb.example/link/x").await.unwrap();
    assert!(fake.calls_to("POST", "/channels/dm-7/messages").pop().unwrap().1["content"].as_str().unwrap().contains("/link/x"));
    // A notification is a direct message too; one that cannot be delivered
    // is the caller's to deal with, not something to say in a channel.
    let notice = Outgoing { text: "**“Fix checkout” failed**".into(), buttons: vec![Button::link("Open", "https://hb.example/s/1")] };
    discord.direct("7", &notice).await.unwrap();
    let (_, sent) = fake.calls_to("POST", "/channels/dm-7/messages").pop().unwrap();
    assert!(sent["content"].as_str().unwrap().contains("failed") && sent.to_string().contains("https://hb.example/s/1"), "{sent}");
    *fake.dms_closed.lock().unwrap() = true;
    assert!(discord.direct("7", &notice).await.is_err());
    discord.whisper(&to, "Link your account: https://hb.example/link/y").await.unwrap();
    let (_, notice) = fake.calls_to("POST", "/channels/100/messages").pop().unwrap();
    let notice = notice["content"].as_str().unwrap();
    assert!(notice.contains("<@7>") && notice.contains("direct message") && !notice.contains("/link/"), "{notice}");

    // Oldest first, for the agent to read.
    let history = discord.history(&place, "none").await.unwrap();
    assert_eq!(history[0], ("Sam".to_string(), "Checkout is failing again".to_string()));
    assert_eq!(discord.email("7").await.unwrap(), None);
    // Only channels a thread can be started in.
    assert_eq!(discord.channels().await.unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["#dev", "#random"]);
}
