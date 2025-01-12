use crate::chat_db::MongoDB;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use mongodb::bson::{doc, Bson, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use actix_web::{web, HttpResponse, Responder};
use log::{error, info};
use uuid::Uuid;
use crate::AppState;
use crate::web_socket_server::ChatMessage;
//chat_server.rs
#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: Uuid,
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
    pub recipient_id: Uuid,
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
        data: web::Data<AppState>, // Access AppState with the MongoDB reference
        web::Path(user_id): web::Path<Uuid>, // user_id from the path
    ) -> impl Responder {
        info!("Fetching chats for user: {}", user_id);

        // Access the MongoDB instance from AppState
        let chat_collection = data.mongodb.db.collection::<Chat>("chats"); // 'mongodb' is already in AppState
        let user_id_bson = Bson::String(user_id.to_string()); // Convert Uuid to Bson string

        // Filter to find all chats where the user is a participant
        let filter = doc! { "participants": user_id_bson };

        // Execute the query
        let mut cursor = match chat_collection.find(filter).await {
            Ok(cursor) => cursor,
            Err(e) => {
                error!("Error fetching chats for user {}: {}", user_id, e);
                return HttpResponse::InternalServerError().body("Error fetching chats");
            }
        };

        let mut chats: Vec<Chat> = Vec::new();

        // Process each document found in the cursor
        while let Some(chat) = cursor.next().await {
            match chat {
                Ok(c) => {
                    info!("Found chat: {}", c.id_chat);
                    chats.push(c); // Collect the chat
                }
                Err(e) => {
                    error!("Error processing chat for user {}: {}", user_id, e);
                    return HttpResponse::InternalServerError().body("Error processing chats");
                }
            }
        }

        // Respond with the list of chats as JSON
        HttpResponse::Ok().json(chats)
    }

    async fn get_messages(
        &self,
        user_id: Uuid,
        chat_id: Uuid,
        since: Option<chrono::DateTime<Utc>>,
    ) -> Vec<MessageResponse> {
        if let Some(chat) = self.get_chat_by_id(chat_id).await {
            if !chat.participants.contains(&user_id) {
                return Vec::new();
            }

            let collection = self.db.db.collection::<Message>("messages");
            let chat_id_bson = Bson::String(chat_id.to_string());
            let filter = if let Some(since_time) = since {
                let since_bson = BsonDateTime::from_millis(since_time.timestamp_millis());
                doc! { "id_chat": chat_id_bson, "created_at": { "$gt": since_bson } }
            } else {
                doc! { "id_chat": chat_id_bson }
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
        let chat_id_bson = Bson::String(chat_id.to_string());

        match collection.find_one(doc! { "_id": chat_id_bson }).await {
            Ok(Some(chat)) => Some(chat),
            Ok(None) => None,
            Err(_) => {
                println!("Failed to retrieve chat: {:?}", chat_id);
                None
            }
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) {
        println!("User {} connected", msg.user_id);
        self.sessions.insert(msg.user_id, msg.addr);
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        println!("User {} disconnected", msg.user_id);
        self.sessions.remove(&msg.user_id);
    }
}

impl Handler<ClientMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: ClientMessage, ctx: &mut Context<Self>) {
        let db = self.db.clone();
        let sessions = self.sessions.clone();
        let sender_id = msg.sender_id;
        let id_chat = msg.id_chat;
        let message_content = msg.message.clone();

        ctx.spawn(async move {
            // Create the message object
            let message = Message {
                id: Uuid::new_v4(),
                id_chat,
                sender_id,
                content: message_content.clone(),
                created_at: Utc::now(),
                msg_type: "text".to_string(),
                attachments: None,
            };

            // Save the message to the database
            let collection = db.db.collection::<Message>("messages");
            let _ = collection.insert_one(&message).await;

            // Retrieve the chat
            let chat_collection = db.db.collection::<Chat>("chats");
            let chat_id_bson = Bson::String(id_chat.to_string());

            // Check if the chat exists
            if let Ok(Some(chat)) = chat_collection.find_one(doc! { "_id": chat_id_bson.clone() }).await {
                // Update last_message_at
                let update = doc! { "$set": { "last_message_at": BsonDateTime::from_millis(Utc::now().timestamp_millis()) } };
                let _ = chat_collection.update_one(doc! { "_id": chat_id_bson }, update).await;

                // Notify participants
                for participant_id in chat.participants {
                    if let Some(addr) = sessions.get(&participant_id) {
                        let _ = addr.do_send(ChatMessage {
                            sender_id,
                            id_chat,
                            message: message_content.clone(),
                        });
                    }
                }
            } else {
                // Chat not found, create a new one
                println!("Chat {} not found, creating a new one.", id_chat);
                let new_chat = Chat {
                    id_chat,
                    participants: vec![sender_id, msg.recipient_id], // Add participants as sender and recipient
                    is_group: false,
                    group_name: None,
                    created_at: Utc::now(),
                    last_message_at: Utc::now(),
                };

                // Insert new chat into the database
                let _ = chat_collection.insert_one(new_chat).await;

                // After creating the chat, you can notify the participants
                if let Some(addr) = sessions.get(&msg.recipient_id) {
                    let _ = addr.do_send(ChatMessage {
                        sender_id,
                        id_chat,
                        message: message_content.clone(),
                    });
                }
            }
        }.into_actor(self));
    }
}

pub async fn create_chat(
    data: web::Data<Addr<ChatServer>>,
    req: web::Json<CreateChatRequest>,
) -> impl Responder {
    if req.participants.len() < 2 {
        return HttpResponse::BadRequest().body("Need at least two participants to create a chat.");
    }

    // Generate a new chat ID
    let chat_id = Uuid::new_v4();

    // Create a `ClientMessage` for the first message
    let client_message = ClientMessage {
        sender_id: req.participants[0], // Assuming the first participant is the sender
        recipient_id: req.participants[1], // Assuming the second participant is the recipient
        id_chat: chat_id,
        message: req.message.clone(),
    };

    // Send the message to the chat server actor
    data.do_send(client_message);

    HttpResponse::Ok().json(serde_json::json!({
        "status": "Chat created",
        "chat_id": chat_id.to_string(),
    }))
}

impl Handler<GetUserChats> for ChatServer {
    type Result = ResponseFuture<Result<UserChatsResponse, ()>>;

    fn handle(&mut self, msg: GetUserChats, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        let user_id = msg.user_id;

        Box::pin(async move {
            let collection = db.db.collection::<Chat>("chats");
            let user_id_bson = Bson::String(user_id.to_string());

            // Find all chats where the user is a participant
            let filter = doc! { "participants": user_id_bson };

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
        let user_id = msg.user_id;
        let chat_id = msg.chat_id;
        let since = msg.since;

        Box::pin(async move {
            let collection = db.db.collection::<Message>("messages");
            let chat_id_bson = Bson::String(chat_id.to_string());
            let filter = if let Some(since_time) = since {
                let since_bson = BsonDateTime::from_millis(since_time.timestamp());
                doc! { "id_chat": chat_id_bson, "created_at": { "$gt": since_bson } }
            } else {
                doc! { "id_chat": chat_id_bson }
            };

            let mut cursor = match collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(_) => return Err(()),
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

            Ok(MessagesResponse { messages })
        })
    }
}
