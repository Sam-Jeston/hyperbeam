//! WebSocket endpoint runners dial into.

use crate::app::App;
use crate::auth::bearer;
use crate::store;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use hyperbeam_core::protocol::{PROTOCOL_VERSION, RunnerMsg, ServerMsg};
use tokio::sync::mpsc;

pub async fn connect(State(app): State<App>, parts: Parts, upgrade: WebSocketUpgrade) -> Response {
    let Some(token) = bearer(&parts) else { return StatusCode::UNAUTHORIZED.into_response() };
    let session = match store::session_by_token(&app.db, &token).await {
        Ok(Some(session)) if app.live.contains(session.id) => session,
        Ok(Some(_)) => return StatusCode::UNAUTHORIZED.into_response(),
        Ok(None) => {
            // Not a session: perhaps a sandbox started to list a harness's models.
            let Some(probe) = app.probes.by_token(&token) else { return StatusCode::UNAUTHORIZED.into_response() };
            return upgrade
                .max_message_size(1024 * 1024)
                .on_upgrade(move |socket| crate::probe::serve(app, probe, token, socket));
        }
        Err(err) => {
            tracing::error!("runner auth lookup failed: {err}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    upgrade
        .max_message_size(8 * 1024 * 1024)
        .on_upgrade(move |socket| serve(app, session, token, socket))
}

async fn serve(app: App, session: store::Session, token: String, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let session_id = session.id;

    // The first frame must be Hello.
    let hello = loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => break serde_json::from_str::<RunnerMsg>(&text).ok(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => break None,
        }
    };
    let Some(RunnerMsg::Hello { protocol, resumed, .. }) = hello else {
        let _ = sink.close().await;
        return;
    };
    if protocol != PROTOCOL_VERSION {
        tracing::warn!("runner for {session_id} speaks protocol {protocol}, expected {PROTOCOL_VERSION}");
        let _ = sink.close().await;
        return;
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let conn = match app.runner_connected(&session, &token, resumed, tx).await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::error!("could not attach runner for {session_id}: {err:#}");
            let _ = sink.close().await;
            app.end_session(session_id, hyperbeam_core::protocol::SessionStatus::Failed, Some(&format!("{err:#}"))).await;
            return;
        }
    };

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let shutdown = matches!(msg, ServerMsg::Shutdown);
            let Ok(text) = serde_json::to_string(&msg) else { continue };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
            if shutdown {
                break;
            }
        }
        let _ = sink.close().await;
    });

    while let Some(frame) = stream.next().await {
        match frame {
            Ok(Message::Text(text)) => match serde_json::from_str::<RunnerMsg>(&text) {
                // Handled in order: the event log must reflect what happened.
                Ok(msg) => app.runner_message(session_id, msg).await,
                Err(err) => tracing::warn!("unreadable runner message for {session_id}: {err}"),
            },
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    app.runner_disconnected(session_id, conn);
    writer.abort();
}
