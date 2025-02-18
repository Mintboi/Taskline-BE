// src/web_socket_server.rs

use actix::{
    Actor, ActorContext, AsyncContext, ContextFutureSpawner,
    Handler, StreamHandler,
};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use log::{info, error};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::chat_server::{
    ChatServer, Connect, Disconnect, CreateMessage, ChatMessage,
};

/// This actor represents a single WebSocket connection (a browser tab).
/// - `user_id`: which user is connected
/// - `chat_server`: reference to the shared ChatServer actor
pub struct WsSession {
    pub user_id: String,
    pub chat_server: actix::Addr<ChatServer>,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("WebSocket started for user_id: {}", self.user_id);

        // Register with ChatServer so we can receive ChatMessage pushes
        self.chat_server.do_send(Connect {
            user_id: self.user_id.clone(),
            chat_id: String::new(), // Not tying user to a single chat yet
            addr: ctx.address().recipient(),
        });
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("WebSocket stopped for user_id: {}", self.user_id);

        // Tell ChatServer we have disconnected
        self.chat_server.do_send(Disconnect {
            user_id: self.user_id.clone(),
        });
    }
}

/// The server can push new messages to us via a `ChatMessage`.
impl Handler<ChatMessage> for WsSession {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, ctx: &mut ws::WebsocketContext<Self>) {
        // We turn it into JSON, so the browser can parse it.
        let json = serde_json::json!({
            "chat_id": msg.chat_id,
            "sender_id": msg.sender_id,
            "content": msg.content
        });
        ctx.text(json.to_string());
    }
}

/// A struct for messages the browser might send us over WebSocket:
#[derive(Deserialize, Serialize)]
struct ClientMsg {
    pub chat_id: String,
    pub content: String,
}

/// Implementation so we can receive messages from the browser
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut ws::WebsocketContext<Self>) {
        match item {
            Ok(ws::Message::Text(txt)) => {
                info!("Received from user {}: {}", self.user_id, txt);

                // Suppose the browser sends JSON like: { "chat_id": "...", "content": "Hello" }
                if let Ok(msg) = serde_json::from_str::<ClientMsg>(&txt) {
                    // We forward to ChatServer to create a message:
                    self.chat_server.do_send(CreateMessage {
                        user_id: self.user_id.clone(),
                        chat_id: msg.chat_id,
                        content: msg.content,
                        attachments: None,
                    });
                }
            }
            Ok(ws::Message::Close(_)) => {
                info!("WsSession: user {} closed", self.user_id);
                ctx.stop();
            }
            _ => {}
        }
    }
}

/// The HTTP handler that upgrades to a WebSocket.
/// e.g. `.route("/ws", web::get().to(ws_index))` in `main.rs`.
pub async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    // We parse `?userId=abc` from the URL query string
    let query = req.uri().query().unwrap_or("");
    let mut user_id = "Anonymous".to_string();
    for piece in query.split('&') {
        if let Some(val) = piece.strip_prefix("userId=") {
            user_id = val.to_string();
        }
    }

    let ws_session = WsSession {
        user_id,
        chat_server: data.chat_server.clone(),
    };

    // This actix call upgrades the HTTP connection to a WebSocket
    ws::start(ws_session, &req, stream)
}
