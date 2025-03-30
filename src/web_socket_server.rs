use actix::{Actor, Handler, StreamHandler, Message, ActorContext, AsyncContext};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use log::{info, error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::chat_server::{ChatServer, Connect, Disconnect, CreateMessage, ChatMessage, WsMessage, RelaySignal};

pub struct WsSession {
    pub user_id: String,
    pub chat_server: actix::Addr<ChatServer>,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("WebSocket started for user_id: {}", self.user_id);
        self.chat_server.do_send(Connect {
            user_id: self.user_id.clone(),
            chat_id: String::new(),
            addr: ctx.address().recipient(),
        });
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        info!("WebSocket stopped for user_id: {}", self.user_id);
        self.chat_server.do_send(Disconnect {
            user_id: self.user_id.clone(),
            addr: ctx.address().recipient(),
        });
    }
}
impl Handler<WsMessage> for WsSession {
    type Result = ();

    fn handle(&mut self, msg: WsMessage, ctx: &mut ws::WebsocketContext<Self>) {
        match msg {
            WsMessage::Chat(chat_msg) => {
                let json = serde_json::json!({
                    "chat_id": chat_msg.chat_id,
                    "sender_id": chat_msg.sender_id,
                    "content": chat_msg.content
                });
                ctx.text(json.to_string());
            }
            WsMessage::Signal(signal_msg) => {
                ctx.text(signal_msg.payload);
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ClientMsg {
    pub chat_id: String,
    pub content: String,
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut ws::WebsocketContext<Self>) {
        match item {
            Ok(ws::Message::Text(txt)) => {
                info!("Received from user {}: {}", self.user_id, txt);
                if let Ok(json_val) = serde_json::from_str::<Value>(&txt) {
                    if json_val.get("signalType").is_some() {
                        let chat_id = json_val.get("chat_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        info!("Relaying signal from user {} for chat {}", self.user_id, chat_id);
                        self.chat_server.do_send(RelaySignal {
                            user_id: self.user_id.clone(),
                            chat_id,
                            message: txt.to_string(),
                        });
                        return;
                    }
                }
                if let Ok(msg) = serde_json::from_str::<ClientMsg>(&txt) {
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

pub async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<crate::app_state::AppState>,
) -> Result<HttpResponse, Error> {
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
    ws::start(ws_session, &req, stream)
}
