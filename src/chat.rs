// src/chat.rs

use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use futures_util::StreamExt;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::app_state::AppState;
use crate::chat_server::{CreateMessage as CreateMessageActor, MessageResponse};

#[derive(Serialize, Deserialize, Clone)]
pub struct Chat {
    #[serde(rename = "_id")]
    pub id_chat: String,
    pub participants: Vec<String>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub last_message_at: chrono::DateTime<Utc>,
}

/// Request body when creating a new chat.
#[derive(Deserialize)]
pub struct CreateChatRequest {
    pub team_id: String,
    pub participants: Vec<String>,
    pub message: String,
}

/// Request body when creating a message (POST /messages/{chat_id}).
#[derive(Deserialize, Debug)]
pub struct CreateMessagePayload {
    // MongoDB ObjectID as a string
    pub sender_id: String,
    pub content: String,
}

/// Document shape for messages in MongoDB (stored as strings).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DBMessage {
    #[serde(rename = "_id")]
    pub id: String,
    pub id_chat: String,
    pub sender_id: String,
    pub content: String,
    pub created_at: chrono::DateTime<Utc>,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub attachments: Option<String>,
}

// -----------------  GET /chats/{user_id}  ----------------- //
pub async fn get_user_chats(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "participants": &*user_id };

    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            return HttpResponse::InternalServerError()
                .body(format!("Error fetching chats: {}", err));
        }
    };

    let mut chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        match chat_res {
            Ok(chat) => chats.push(chat),
            Err(err) => {
                return HttpResponse::InternalServerError()
                    .body(format!("Error iterating over chats: {}", err));
            }
        }
    }
    HttpResponse::Ok().json(chats)
}

// -----------------  GET /messages/{chat_id}  ----------------- //
pub async fn get_messages(
    data: web::Data<AppState>,
    chat_id: web::Path<String>,
) -> impl Responder {
    let chat_id_str = chat_id.into_inner();

    // Find all DBMessage docs where `id_chat == chat_id_str`
    let messages_collection = data.mongodb.db.collection::<DBMessage>("messages");
    let filter = doc! { "id_chat": &chat_id_str };

    let mut cursor = match messages_collection.find(filter).await {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Error fetching messages: {}", e));
        }
    };

    let mut all_msgs = Vec::new();
    while let Some(res) = cursor.next().await {
        match res {
            Ok(msg_doc) => all_msgs.push(msg_doc),
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .body(format!("Error iterating messages: {}", e));
            }
        }
    }

    #[derive(Serialize)]
    struct MsgResponse {
        messages: Vec<DBMessage>,
    }
    HttpResponse::Ok().json(MsgResponse { messages: all_msgs })
}

// -----------------  POST /chats  ----------------- //
pub async fn create_chat(
    data: web::Data<AppState>,
    chat_info: web::Json<CreateChatRequest>,
) -> impl Responder {
    let new_chat_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let new_chat = Chat {
        id_chat: new_chat_id.clone(),
        participants: chat_info.participants.clone(),
        is_group: chat_info.participants.len() > 2,
        group_name: None,
        created_at: now,
        last_message_at: now,
    };

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    match chats_collection.insert_one(&new_chat).await {
        Ok(_) => HttpResponse::Ok().json(&new_chat),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to create chat: {}", e)),
    }
}

// -----------------  GET /chats/search/{user_id}  ----------------- //
pub async fn search_chats(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let _q = query.get("q").unwrap_or(&"".to_string()).to_lowercase();
    let user_id_str = user_id.into_inner();

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "participants": &user_id_str };

    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Error fetching chats: {}", e));
        }
    };

    let mut result_chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        if let Ok(chat) = chat_res {
            result_chats.push(chat);
        }
    }
    HttpResponse::Ok().json(result_chats)
}

// -----------------  DELETE /chats/{chat_id}  ----------------- //
pub async fn delete_chat(
    data: web::Data<AppState>,
    chat_id: web::Path<String>,
    req: HttpRequest,
) -> impl Responder {
    let chat_id_str = chat_id.into_inner();

    // The user_id from the token
    let user_id = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "_id": &chat_id_str };

    let chat = match chats_collection.find_one(filter.clone()).await {
        Ok(Some(c)) => c,
        Ok(None) => return HttpResponse::NotFound().body("Chat not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching chat: {}", e)),
    };

    if !chat.participants.iter().any(|p| p == &user_id) {
        return HttpResponse::Unauthorized().body("Not a participant in the chat");
    }

    match chats_collection.delete_one(filter).await {
        Ok(_) => {
            let messages_collection = data.mongodb.db.collection::<DBMessage>("messages");
            let _ = messages_collection.delete_many(doc! { "id_chat": &chat_id_str }).await;
            HttpResponse::Ok().body("Chat deleted successfully")
        },
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting chat: {}", e)),
    }
}

// -----------------  POST /messages/{chat_id}  ----------------- //
pub async fn create_message(
    _req: HttpRequest,
    data: web::Data<AppState>,
    chat_id: web::Path<String>,
    payload: web::Json<CreateMessagePayload>,
) -> impl Responder {
    log::info!(
        "Received create_message request with chat_id: {} and payload: {:?}",
        chat_id,
        payload
    );

    let chat_id_str = chat_id.into_inner();

    // Optional: Confirm user is in chat doc
    // This check ensures the doc has participants array containing the same string
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    match chats_collection
        .find_one(doc! { "_id": &chat_id_str, "participants": &payload.sender_id })
        .await
    {
        Ok(Some(_)) => { /* user is indeed a participant */ }
        _ => {
            log::error!("User {} not in chat {}", payload.sender_id, chat_id_str);
            return HttpResponse::BadRequest().body("You are not a participant in this chat");
        }
    }

    // Build the actor message with all strings
    let create_msg = crate::chat_server::CreateMessage {
        user_id: payload.sender_id.clone(),
        chat_id: chat_id_str.clone(),
        content: payload.content.clone(),
        attachments: None,
    };

    log::info!(
        "Sending create message to chat server actor: user_id: {}, chat_id: {}, content: {}",
        create_msg.user_id,
        create_msg.chat_id,
        create_msg.content
    );

    let chat_server = data.chat_server.clone();
    match chat_server.send(create_msg).await {
        // If actor returns Ok(...) we return 200 + JSON
        Ok(Ok(msg_response)) => {
            log::info!("Message created successfully: {:?}", msg_response);
            HttpResponse::Ok().json(msg_response)
        }
        // If actor returns Err(()) => membership / chat not found / DB error
        Ok(Err(_)) => {
            log::error!("Chat server actor returned error: ()");
            HttpResponse::InternalServerError().body("Failed to create message")
        }
        // If we couldn't even send the message
        Err(e) => {
            log::error!("Failed sending to chat server actor: {:?}", e);
            HttpResponse::InternalServerError().body("Failed to create message")
        }
    }
}
