use crate::models::WsEvent;
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;

pub type WsTx = broadcast::Sender<WsEvent>;

pub fn create_broadcaster() -> (WsTx, broadcast::Receiver<WsEvent>) {
    broadcast::channel(256)
}

pub async fn handle_ws(socket: WebSocket, tx: Arc<WsTx>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {
            // Client messages ignored for now
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
