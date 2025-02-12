// File: message.rs

use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use chrono::Utc;
use futures_util::StreamExt;
use mongodb::bson::{doc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::app_state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id")]
    pub id: Uuid,
    pub chat_id: Uuid,
    pub sender_id: Uuid,
    pub content: String,
    pub created_at: chrono::DateTime<Utc>,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub attachments: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub attachments: Option<Vec<String>>,
}

pub async fn send_message(
    req: HttpRequest,
    data: web::Data<AppState>,
    chat_id: web::Path<Uuid>,
    msg_info: web::Json<SendMessageRequest>,
) -> impl Responder {
    let chat_id = chat_id.into_inner();
    // Assume the authenticated user is stored as a String (UUID string) in extensions.
    let current_user = if let Some(user) = req.extensions().get::<String>() {
        user.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    let chat_users_collection = data.mongodb.db.collection::<mongodb::bson::Document>("chat_users");
    let filter = doc! { "chat_id": chat_id.to_string(), "user_id": current_user.clone() };
    match chat_users_collection.find_one(filter).await {
        Ok(Some(_)) => {},
        Ok(None) => return HttpResponse::Unauthorized().body("User is not a member of this chat"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error verifying chat membership: {}", e)),
    }
    let new_message = Message {
        id: Uuid::new_v4(),
        chat_id,
        sender_id: Uuid::parse_str(&current_user).unwrap_or(Uuid::nil()),
        content: msg_info.content.clone(),
        created_at: Utc::now(),
        msg_type: "text".to_string(),
        attachments: msg_info.attachments.clone(),
    };
    let messages_collection = data.mongodb.db.collection::<Message>("messages");
    match messages_collection.insert_one(&new_message).await {
        Ok(_) => HttpResponse::Ok().json(new_message),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error sending message: {}", e)),
    }
}

pub async fn fetch_messages(
    req: HttpRequest,
    data: web::Data<AppState>,
    chat_id: web::Path<Uuid>,
) -> impl Responder {
    let chat_id = chat_id.into_inner();
    let current_user = if let Some(user) = req.extensions().get::<String>() {
        user.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    let chat_users_collection = data.mongodb.db.collection::<mongodb::bson::Document>("chat_users");
    let filter = doc! { "chat_id": chat_id.to_string(), "user_id": current_user };
    match chat_users_collection.find_one(filter).await {
        Ok(Some(_)) => {},
        Ok(None) => return HttpResponse::Unauthorized().body("User is not a member of this chat"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error verifying chat membership: {}", e)),
    }
    let messages_collection = data.mongodb.db.collection::<Message>("messages");
    let filter = doc! { "chat_id": chat_id.to_string() };
    let mut cursor = match messages_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching messages: {}", e)),
    };
    let mut messages = Vec::new();
    while let Some(result) = cursor.next().await {
        if let Ok(msg) = result {
            messages.push(msg);
        }
    }
    HttpResponse::Ok().json(messages)
}
