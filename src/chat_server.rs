use crate::chat_db::MongoDB;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use log::{error, info};

use crate::app_state::AppState;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ChatMessage {
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct SignalMessage {
    pub payload: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum WsMessage {
    Chat(ChatMessage),
    Signal(SignalMessage),
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: String,
    pub chat_id: String,
    pub addr: Recipient<WsMessage>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub user_id: String,
}

#[derive(Message)]
#[rtype(result = "Result<MessageResponse, ()>")]
pub struct CreateMessage {
    pub user_id: String,
    pub chat_id: String,
    pub content: String,
    pub attachments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub id_chat: String,
    pub sender_id: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub msg_type: String,
    pub attachments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chat {
    #[serde(rename = "_id")]
    pub id_chat: String,
    pub participants: Vec<String>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_message_at: DateTime<Utc>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct RelaySignal {
    pub user_id: String,
    pub chat_id: String,
    pub message: String,
}

pub struct ChatServer {
    sessions: HashMap<String, Recipient<WsMessage>>,
    db: Arc<MongoDB>,
}

impl ChatServer {
    pub fn new(db: Arc<MongoDB>) -> Self {
        ChatServer {
            sessions: HashMap::new(),
            db,
        }
    }

    async fn get_chat_by_id(&self, chat_id_str: &str) -> Option<Chat> {
        let collection = self.db.db.collection::<Chat>("chats");
        match collection.find_one(doc! { "_id": chat_id_str }).await {
            Ok(Some(chat)) => Some(chat),
            _ => None,
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) {
        info!("User {} connected (WS). ChatID param: {}", msg.user_id, msg.chat_id);
        self.sessions.insert(msg.user_id.clone(), msg.addr);
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        info!("User {} disconnected (WS)", msg.user_id);
        self.sessions.remove(&msg.user_id);
    }
}

impl Handler<CreateMessage> for ChatServer {
    type Result = ResponseFuture<Result<MessageResponse, ()>>;

    fn handle(&mut self, msg: CreateMessage, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        let sessions_map = self.sessions.clone();
        Box::pin(async move {
            let chats_coll = db.db.collection::<Chat>("chats");
            let chat_doc = match chats_coll.find_one(doc! { "_id": &msg.chat_id }).await {
                Ok(Some(c)) => c,
                _ => return Err(()),
            };
            if !chat_doc.participants.contains(&msg.user_id) {
                return Err(());
            }
            let now = Utc::now();
            let new_msg_id = uuid::Uuid::new_v4().to_string();
            #[derive(Serialize)]
            struct DBMessage {
                #[serde(rename = "_id")]
                pub id: String,
                pub id_chat: String,
                pub sender_id: String,
                pub content: String,
                pub created_at: DateTime<Utc>,
                #[serde(rename = "type")]
                pub msg_type: String,
                pub attachments: Option<String>,
            }
            let new_db_msg = DBMessage {
                id: new_msg_id.clone(),
                id_chat: msg.chat_id.clone(),
                sender_id: msg.user_id.clone(),
                content: msg.content.clone(),
                created_at: now,
                msg_type: "text".to_string(),
                attachments: msg.attachments.clone(),
            };
            let messages_coll = db.db.collection::<DBMessage>("messages");
            if messages_coll.insert_one(new_db_msg).await.is_err() {
                return Err(());
            }
            for participant_id in &chat_doc.participants {
                if participant_id != &msg.user_id {
                    if let Some(ws_addr) = sessions_map.get(participant_id) {
                        ws_addr.do_send(WsMessage::Chat(ChatMessage {
                            chat_id: msg.chat_id.clone(),
                            sender_id: msg.user_id.clone(),
                            content: msg.content.clone(),
                        }));
                    }
                }
            }
            Ok(MessageResponse {
                id: new_msg_id,
                id_chat: msg.chat_id,
                sender_id: msg.user_id,
                content: msg.content,
                created_at: now,
                msg_type: "text".to_string(),
                attachments: msg.attachments,
            })
        })
    }
}

impl Handler<RelaySignal> for ChatServer {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: RelaySignal, _ctx: &mut Context<Self>) -> Self::Result {
        let sessions_map = self.sessions.clone();
        let db = self.db.clone();
        Box::pin(async move {
            let chats_coll = db.db.collection::<Chat>("chats");
            if let Ok(Some(chat_doc)) = chats_coll.find_one(doc! { "_id": &msg.chat_id }).await {
                for participant in chat_doc.participants {
                    if participant != msg.user_id {
                        if let Some(addr) = sessions_map.get(&participant) {
                            addr.do_send(WsMessage::Signal(SignalMessage {
                                payload: msg.message.clone(),
                            }));
                        }
                    }
                }
            } else {
                for (uid, addr) in sessions_map.iter() {
                    if uid != &msg.user_id {
                        addr.do_send(WsMessage::Signal(SignalMessage {
                            payload: msg.message.clone(),
                        }));
                    }
                }
            }
        })
    }
}

use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use serde_json::Value;

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

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("WebSocket stopped for user_id: {}", self.user_id);
        self.chat_server.do_send(Disconnect {
            user_id: self.user_id.clone(),
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
    data: web::Data<AppState>,
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
