// File: web_socket_server.rs

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ChatMessage {
    pub sender_id: Uuid,
    pub chat_id: Uuid,
    pub message: String,
}

// Updated Connect struct now includes the chat_id field.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: Uuid,
    pub chat_id: Uuid,
    pub addr: Recipient<ChatMessage>,
}

// Updated ClientMessage struct without the non-existent `recipient_id` field.
#[derive(Message)]
#[rtype(result = "()")]
pub struct ClientMessage {
    pub sender_id: Uuid,
    pub chat_id: Uuid,
    pub message: String,
}
