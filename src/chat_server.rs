// src/chat_server.rs

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

/// A push message from `ChatServer` to each user’s `WsSession` (server -> client).
#[derive(Message)]
#[rtype(result = "()")]
pub struct ChatMessage {
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
}

/// A message to register a WebSocket connection with the `ChatServer`.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: String,
    pub chat_id: String,  // if you want them to join a specific chat, otherwise can be empty
    pub addr: Recipient<ChatMessage>,
}

/// A message to tell `ChatServer` a user’s WebSocket is gone.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub user_id: String,
}

/// A request to create a new chat message. Called by your `/messages/{chat_id}` REST route
/// or possibly from a WebSocket client (the latter would require your WsSession to do_send(CreateMessage)).
#[derive(Message)]
#[rtype(result = "Result<MessageResponse, ()>")]
pub struct CreateMessage {
    pub user_id: String,
    pub chat_id: String,
    pub content: String,
    pub attachments: Option<String>,
}

/// A response containing the newly created message data.
#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,           // The message’s string ID
    pub id_chat: String,      // Which chat it belongs to
    pub sender_id: String,    // Who sent it
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub msg_type: String,
    pub attachments: Option<String>,
}

/// For completeness: A “Chat” doc in Mongo, storing `_id` and participants as strings.
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

/// The ChatServer actor holds a `sessions` map of user_id -> WsSession address.
pub struct ChatServer {
    /// For real-time, store each user’s WebSocket address if connected.
    sessions: HashMap<String, Recipient<ChatMessage>>,
    /// A handle to your MongoDB so you can read/write chat & message docs.
    db: Arc<MongoDB>,
}

impl ChatServer {
    pub fn new(db: Arc<MongoDB>) -> Self {
        ChatServer {
            sessions: HashMap::new(),
            db,
        }
    }

    /// Helper to load a “Chat” doc by `_id`.
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

// ---------------------------------------------------------------------
// Handler for Connect
// ---------------------------------------------------------------------
impl Handler<Connect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) {
        info!("User {} connected (WebSocket). ChatID param: {}", msg.user_id, msg.chat_id);

        // Insert the user’s WsSession address into our sessions map
        self.sessions.insert(msg.user_id.clone(), msg.addr);
    }
}

// ---------------------------------------------------------------------
// Handler for Disconnect
// ---------------------------------------------------------------------
impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        info!("User {} disconnected (WebSocket)", msg.user_id);
        self.sessions.remove(&msg.user_id);
    }
}

// ---------------------------------------------------------------------
// Handler for CreateMessage (the main piece for new chat messages)
// ---------------------------------------------------------------------
impl Handler<CreateMessage> for ChatServer {
    type Result = ResponseFuture<Result<MessageResponse, ()>>;

    fn handle(&mut self, msg: CreateMessage, _: &mut Context<Self>) -> Self::Result {
        let db = self.db.clone();
        let sessions_map = self.sessions.clone(); // so we can broadcast afterwards

        Box::pin(async move {
            // 1) Load chat from DB
            let chats_coll = db.db.collection::<Chat>("chats");
            let chat_doc = match chats_coll
                .find_one(doc! { "_id": &msg.chat_id })
                .await
            {
                Ok(Some(c)) => c,
                _ => return Err(()),
            };

            // 2) Check membership
            if !chat_doc.participants.contains(&msg.user_id) {
                // user not in chat => fail
                return Err(());
            }

            // 3) Insert new message into "messages" collection
            let now = Utc::now();
            let new_msg_id = uuid::Uuid::new_v4().to_string();

            // If your messages are stored as raw doc, or with a struct:
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
            if messages_coll.insert_one(&new_db_msg).await.is_err() {
                return Err(());
            }

            // 4) Broadcast via WebSocket to all participants who are connected
            for participant_id in &chat_doc.participants {
                if let Some(ws_addr) = sessions_map.get(participant_id) {
                    // Send them a ChatMessage
                    ws_addr.do_send(ChatMessage {
                        chat_id: msg.chat_id.clone(),
                        sender_id: msg.user_id.clone(),
                        content: msg.content.clone(),
                    });
                }
            }

            // 5) Return a “MessageResponse” to the REST caller
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
