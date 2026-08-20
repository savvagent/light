//! Authenticated WebSocket endpoint (`GET /ws`).

use std::collections::HashMap;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use light_factory_auth::AuthError;
use light_factory_auth::store::User;
use light_factory_protocol::wire::{ClientMessage, ServerMessage};

use crate::auth_extract::bearer_token;
use crate::error::ApiError;
use crate::routes::to_user_view;
use crate::state::AppState;

fn unauthorized() -> Response {
    ApiError::from(AuthError::InvalidSession).into_response()
}

/// Upgrade an authenticated WebSocket. The token may arrive via the
/// `Authorization: Bearer` header or, for browser clients that cannot set
/// headers, the `?token=` query parameter.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let token = bearer_token(&headers).or_else(|| params.get("token").cloned());
    let Some(token) = token else {
        return unauthorized();
    };

    let user = match state.auth.authenticate(&token).await {
        Ok(user) => user,
        Err(_) => return unauthorized(),
    };

    ws.on_upgrade(move |socket| handle_socket(socket, user))
}

async fn handle_socket(socket: WebSocket, user: User) {
    let (mut sender, mut receiver) = socket.split();

    let ready = ServerMessage::Ready {
        user: to_user_view(&user),
    };
    if send_text(&mut sender, &ready).await.is_err() {
        return;
    }

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                let client: ClientMessage = match serde_json::from_str(text.as_str()) {
                    Ok(parsed) => parsed,
                    Err(_) => {
                        let _ = send_text(
                            &mut sender,
                            &ServerMessage::Error {
                                code: "bad_message".to_string(),
                                message: "could not parse message".to_string(),
                            },
                        )
                        .await;
                        continue;
                    }
                };
                match client {
                    ClientMessage::Ping { nonce } => {
                        if send_text(&mut sender, &ServerMessage::Pong { nonce })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn send_text(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &ServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).expect("ServerMessage is serializable");
    sender
        .send(Message::Text(Utf8Bytes::from(text)))
        .await
        .map(|_| ())
}
