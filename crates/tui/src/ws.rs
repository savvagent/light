//! WebSocket client: connect, split, and pump frames into the event loop.

use futures_util::{SinkExt, StreamExt};
use light_factory_protocol::wire::{ClientMessage, ServerMessage};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::app::UiEvent;
use crate::config::Config;

/// Connect to `GET /ws?token=...` and forward inbound [`ServerMessage`]s to the
/// app as [`UiEvent::Server`] events. Returns the outbound sender used to push
/// [`ClientMessage`]s, or an error if the upgrade fails.
pub async fn connect(
    config: &Config,
    token: &str,
    events: &mpsc::UnboundedSender<UiEvent>,
) -> anyhow::Result<mpsc::UnboundedSender<ClientMessage>> {
    let url = format!("{}?token={}", config.ws_url, token);
    let (stream, _resp) = connect_async(url).await?;
    let (mut sink, mut source) = stream.split();

    let (tx, mut rx) = mpsc::unbounded_channel::<ClientMessage>();

    let reader_events = events.clone();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = source.next().await {
            let parsed = match msg {
                Message::Text(text) => serde_json::from_str::<ServerMessage>(text.as_str())
                    .unwrap_or(ServerMessage::Error {
                        code: "bad_message".into(),
                        message: "could not parse server message".into(),
                    }),
                Message::Close(_) => {
                    let _ = reader_events.send(UiEvent::Server(ServerMessage::Error {
                        code: "ws_closed".into(),
                        message: "server closed the connection".into(),
                    }));
                    return;
                }
                _ => continue,
            };
            if reader_events.send(UiEvent::Server(parsed)).is_err() {
                return;
            }
        }
        let _ = reader_events.send(UiEvent::Server(ServerMessage::Error {
            code: "ws_closed".into(),
            message: "connection closed".into(),
        }));
    });

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = serde_json::to_string(&msg).expect("ClientMessage is serializable");
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    Ok(tx)
}
