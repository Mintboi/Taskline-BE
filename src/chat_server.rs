use crate::chat_db::MongoDB;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use actix_web::{web, HttpResponse, Responder};
use log::{error, info};
use uuid::Uuid;
use crate::app_state::AppState;
use crate::web_socket_server::{ChatMessage, Connect as WSConnect, ClientMessage as WSClientMessage};

#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: Uuid,
    pub chat_id: Uuid,
    pub addr: Recipient<ChatMessage>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub user_id: Uuid,
}

#[derive(Message)]
#[rtype(result = "Result<UserChatsResponse, ()>")]
pub struct GetUserChats {
    pub user_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserChatsResponse {
    pub chats: Vec<Chat>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientMessage {
    pub sender_id: Uuid,
    pub id_chat: Uuid,
    pub message: String,
}

#[derive(Message)]
#[rtype(result = "Result<MessagesResponse, ()>")]
pub struct GetMessages {
    pub user_id: Uuid,
    pub chat_id: Uuid,
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id")]
    pub id: Uuid,
    pub id_chat: Uuid,
    pub sender_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub attachments: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateChatRequest {
    team_id: String,
    participants: Vec<Uuid>,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Chat {
    #[serde(rename = "_id")]
    pub id_chat: Uuid,
    pub participants: Vec<Uuid>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_message_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub id_chat: Uuid,
    pub sender_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub msg_type: String,
    pub attachments: Option<String>,
}

#[derive(Message, Serialize)]
#[rtype(result = "()")]
pub struct MessagesResponse {
    pub messages: Vec<MessageResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatUser {
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub joined_at: DateTime<Utc>,
}

pub struct ChatServer {
    sessions: HashMap<Uuid, Recipient<ChatMessage>>,
    db: Arc<MongoDB>,
}

impl ChatServer {
    pub fn new(db: Arc<MongoDB>) -> Self {
        ChatServer {
            sessions: HashMap::new(),
            db,
        }
    }

    async fn save_message(&self, message: &Message) {
        let collection = self.db.db.collection::<Message>("messages");
        let _ = collection.insert_one(message).await;
    }

    pub async fn get_user_chats(
        data: web::Data<AppState>,
        user_id: web::Path<Uuid>,
    ) -> impl Responder {
        let user_id = user_id.into_inner();
        info!("Fetching chats for user: {}", user_id);
        let chat_collection = data.mongodb.db.collection::<Chat>("chats");
        let filter = doc! { "participants": user_id.to_string() };

        let mut cursor = match chat_collection.find(filter).await {
            Ok(cursor) => cursor,
            Err(e) => {
                error!("Error fetching chats for user {}: {}", user_id, e);
                return HttpResponse::InternalServerError().body("Error fetching chats");
            }
        };

        let mut chats: Vec<Chat> = Vec::new();
        while let Some(chat) = cursor.next().await {
            match chat {
                Ok(c) => chats.push(c),
                Err(e) => {
                    error!("Error processing chat for user {}: {}", user_id, e);
                    return HttpResponse::InternalServerError().body("Error processing chats");
                }
            }
        }
        HttpResponse::Ok().json(chats)
    }

    async fn get_messages(
        &self,
        user_id: Uuid,
        chat_id: Uuid,
        since: Option<DateTime<Utc>>,
    ) -> Vec<MessageResponse> {
        if let Some(chat) = self.get_chat_by_id(chat_id).await {
            if !chat.participants.contains(&user_id) {
                return Vec::new();
            }
            let collection = self.db.db.collection::<Message>("messages");
            let filter = if let Some(since_time) = since {
                let since_bson = BsonDateTime::from_millis(since_time.timestamp_millis());
                doc! { "id_chat": chat_id.to_string(), "created_at": { "$gt": since_bson } }
            } else {
                doc! { "id_chat": chat_id.to_string() }
            };

            let mut cursor = match collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(_) => return Vec::new(),
            };

            let mut messages = Vec::new();
            while let Some(result) = cursor.next().await {
                if let Ok(msg) = result {
                    messages.push(MessageResponse {
                        id: msg.id,
                        id_chat: msg.id_chat,
                        sender_id: msg.sender_id,
                        content: msg.content,
                        created_at: msg.created_at,
                        msg_type: msg.msg_type,
                        attachments: msg.attachments,
                    });
                }
            }
            messages
        } else {
            Vec::new()
        }
    }

    async fn get_chat_by_id(&self, chat_id: Uuid) -> Option<Chat> {
        let collection = self.db.db.collection::<Chat>("chats");
        match collection.find_one(doc! { "_id": chat_id.to_string() }).await {
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
        let db = self.db.clone();
        let allowed = {
            let collection = db.db.collection::<ChatUser>("chat_users");
            futures::executor::block_on(async {
                let filter = doc! {
                    "chat_id": msg.chat_id.to_string(),
                    "user_id": msg.user_id.to_string(),
                };
                match collection.find_one(filter).await {
                    Ok(Some(_)) => true,
                    _ => false,
                }
            })
        };

        if allowed {
            info!("User {} connected to chat {}", msg.user_id, msg.chat_id);
            self.sessions.insert(msg.user_id, msg.addr);
        } else {
            error!("Connect denied: User {} is not a member of chat {}", msg.user_id, msg.chat_id);
        }
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        self.sessions.remove(&msg.user_id);
    }
}

impl Handler<ClientMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: ClientMessage, ctx: &mut Context<Self>) {
        let db = self.db.clone();
        let sessions = self.sessions.clone();
        ctx.spawn(async move {
            let collection = db.db.collection::<Chat>("chats");
            match collection.find_one(doc! { "_id": msg.id_chat.to_string() }).await {
                Ok(Some(chat)) => {
                    if !chat.participants.contains(&msg.sender_id) {
                        error!("Message rejected: Sender {} is not a member of chat {}", msg.sender_id, msg.id_chat);
                        return;
                    }
                    let message = Message {
                        id: Uuid::new_v4(),
                        id_chat: msg.id_chat,
                        sender_id: msg.sender_id,
                        content: msg.message.clone(),
                        created_at: Utc::now(),
                        msg_type: "text".to_string(),
                        attachments: None,
                    };
                    let msg_collection = db.db.collection::<Message>("messages");
                    let _ = msg_collection.insert_one(&message).await;
                    for participant in chat.participants {
                        if let Some(addr) = sessions.get(&participant) {
                            // Here we now send a ChatMessage with the correct field name `chat_id`
                            let _ = addr.do_send(ChatMessage {
                                sender_id: msg.sender_id,
                                chat_id: msg.id_chat,
                                message: msg.message.clone(),
                            });
                        }
                    }
                }
                _ => {
                    error!("Chat {} not found; message dropped", msg.id_chat);
                }
            }
        }.into_actor(self));
    }
}

impl Handler<GetUserChats> for ChatServer {
    type Result = ResponseFuture<Result<UserChatsResponse, ()>>;

    fn handle(&mut self, msg: GetUserChats, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        let user_id = msg.user_id;
        Box::pin(async move {
            let collection = db.db.collection::<Chat>("chats");
            let filter = doc! { "participants": user_id.to_string() };
            let mut cursor = match collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(_) => return Err(()),
            };
            let mut chats = Vec::new();
            while let Some(result) = cursor.next().await {
                if let Ok(chat) = result {
                    chats.push(chat);
                }
            }
            Ok(UserChatsResponse { chats })
        })
    }
}

impl Handler<GetMessages> for ChatServer {
    type Result = ResponseFuture<Result<MessagesResponse, ()>>;

    fn handle(&mut self, msg: GetMessages, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            let chat_collection = db.db.collection::<Chat>("chats");
            let chat_doc = chat_collection.find_one(doc! { "_id": msg.chat_id.to_string() })
                .await.map_err(|_| ())?;
            let chat = chat_doc.ok_or(())?;
            if !chat.participants.contains(&msg.user_id) {
                return Err(());
            }
            let messages = {
                let collection = db.db.collection::<Message>("messages");
                let filter = if let Some(since) = msg.since {
                    let since_bson = BsonDateTime::from_millis(since.timestamp_millis());
                    doc! { "id_chat": msg.chat_id.to_string(), "created_at": { "$gt": since_bson } }
                } else {
                    doc! { "id_chat": msg.chat_id.to_string() }
                };
                let mut cursor = collection.find(filter).await.map_err(|_| ())?;
                let mut msgs = Vec::new();
                while let Some(result) = cursor.next().await {
                    if let Ok(msg) = result {
                        msgs.push(MessageResponse {
                            id: msg.id,
                            id_chat: msg.id_chat,
                            sender_id: msg.sender_id,
                            content: msg.content,
                            created_at: msg.created_at,
                            msg_type: msg.msg_type,
                            attachments: msg.attachments,
                        });
                    }
                }
                msgs
            };
            Ok(MessagesResponse { messages })
        })
    }
}

// CRUD operations for messages

#[derive(Message)]
#[rtype(result = "Result<MessageResponse, ()>")]
pub struct CreateMessage {
    pub user_id: Uuid,
    pub chat_id: Uuid,
    pub content: String,
    pub attachments: Option<String>,
}

#[derive(Message)]
#[rtype(result = "Result<MessageResponse, ()>")]
pub struct UpdateMessage {
    pub user_id: Uuid,
    pub message_id: Uuid,
    pub new_content: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), ()>")]
pub struct DeleteMessage {
    pub user_id: Uuid,
    pub message_id: Uuid,
}

impl Handler<CreateMessage> for ChatServer {
    type Result = ResponseFuture<Result<MessageResponse, ()>>;

    fn handle(&mut self, msg: CreateMessage, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            let chat_collection = db.db.collection::<Chat>("chats");
            let chat_doc = chat_collection.find_one(doc! { "_id": msg.chat_id.to_string() })
                .await.map_err(|_| ())?;
            let chat = chat_doc.ok_or(())?;
            if !chat.participants.contains(&msg.user_id) {
                return Err(());
            }
            let new_message = Message {
                id: Uuid::new_v4(),
                id_chat: msg.chat_id,
                sender_id: msg.user_id,
                content: msg.content,
                created_at: Utc::now(),
                msg_type: "text".to_string(),
                attachments: msg.attachments,
            };
            let messages_collection = db.db.collection::<Message>("messages");
            messages_collection.insert_one(&new_message).await.map_err(|_| ())?;
            Ok(MessageResponse {
                id: new_message.id,
                id_chat: new_message.id_chat,
                sender_id: new_message.sender_id,
                content: new_message.content,
                created_at: new_message.created_at,
                msg_type: new_message.msg_type,
                attachments: new_message.attachments,
            })
        })
    }
}

impl Handler<UpdateMessage> for ChatServer {
    type Result = ResponseFuture<Result<MessageResponse, ()>>;

    fn handle(&mut self, msg: UpdateMessage, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            let messages_collection = db.db.collection::<Message>("messages");
            let filter = doc! { "_id": msg.message_id.to_string() };
            let message_doc = messages_collection.find_one(filter.clone()).await.map_err(|_| ())?;
            let mut message = message_doc.ok_or(())?;
            if message.sender_id != msg.user_id {
                return Err(());
            }
            message.content = msg.new_content;
            messages_collection.replace_one(filter, &message).await.map_err(|_| ())?;
            Ok(MessageResponse {
                id: message.id,
                id_chat: message.id_chat,
                sender_id: message.sender_id,
                content: message.content,
                created_at: message.created_at,
                msg_type: message.msg_type,
                attachments: message.attachments,
            })
        })
    }
}

impl Handler<DeleteMessage> for ChatServer {
    type Result = ResponseFuture<Result<(), ()>>;

    fn handle(&mut self, msg: DeleteMessage, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        Box::pin(async move {
            let messages_collection = db.db.collection::<Message>("messages");
            let filter = doc! { "_id": msg.message_id.to_string() };
            let message_doc = messages_collection.find_one(filter.clone()).await.map_err(|_| ())?;
            let message = message_doc.ok_or(())?;
            if message.sender_id != msg.user_id {
                return Err(());
            }
            messages_collection.delete_one(filter).await.map_err(|_| ())?;
            Ok(())
        })
    }
}
