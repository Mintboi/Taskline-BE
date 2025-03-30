use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use futures_util::StreamExt;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::app_state::AppState;
use crate::chat_server::{CreateMessage as CreateMessageActor};

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

#[derive(Deserialize)]
pub struct CreateChatRequest {
    pub team_id: String,
    pub participants: Vec<String>,
    pub group_name: Option<String>,
    pub message: String,
}

#[derive(Deserialize, Debug)]
pub struct CreateMessagePayload {
    pub sender_id: String,
    pub content: String,
}

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

// ----------------------------------------------------------------------
// GET /chats/{user_id} => list all chats in which that user participates
// ----------------------------------------------------------------------
pub async fn get_user_chats(
    data: web::Data<AppState>,
    user_id_path: web::Path<String>,
) -> impl Responder {
    let user_id_str = user_id_path.into_inner(); // store in a binding
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");

    let filter = doc! { "participants": &user_id_str };
    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            return HttpResponse::InternalServerError().body(format!("Error fetching chats: {}", err));
        }
    };

    let mut chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        match chat_res {
            Ok(chat_doc) => chats.push(chat_doc),
            Err(err) => {
                return HttpResponse::InternalServerError()
                    .body(format!("Error iterating over chats: {}", err));
            }
        }
    }
    HttpResponse::Ok().json(chats)
}

// ----------------------------------------------------------------------
// GET /chats/get/{chat_id} => fetch a single chat document
//    (Use this to retrieve group_name or is_group, etc.)
// ----------------------------------------------------------------------
pub async fn get_single_chat(
    data: web::Data<AppState>,
    chat_id_path: web::Path<String>,
    req: HttpRequest,
) -> impl Responder {
    // Optionally ensure the user is authorized:
    let user_id_opt = req.extensions().get::<String>().cloned();
    if user_id_opt.is_none() {
        return HttpResponse::Unauthorized().body("Unauthorized");
    }
    let user_id = user_id_opt.unwrap();
    let chat_id_str = chat_id_path.into_inner();

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    match chats_collection.find_one(doc! { "_id": &chat_id_str }).await {
        Ok(Some(chat_doc)) => {
            // if you want to ensure user is a participant:
            if !chat_doc.participants.contains(&user_id) {
                return HttpResponse::Forbidden().body("You are not a participant of this chat.");
            }
            HttpResponse::Ok().json(chat_doc)
        }
        Ok(None) => HttpResponse::NotFound().body("No chat found for that ID"),
        Err(e) => HttpResponse::InternalServerError().body(format!("DB error: {}", e)),
    }
}

// ----------------------------------------------------------------------
// GET /messages/{chat_id} => fetch all messages for a given chat
// ----------------------------------------------------------------------
pub async fn get_messages(
    data: web::Data<AppState>,
    chat_id_path: web::Path<String>,
) -> impl Responder {
    let chat_id_str = chat_id_path.into_inner();
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

// ----------------------------------------------------------------------
// POST /chats => create a new chat
// ----------------------------------------------------------------------
pub async fn create_chat(
    data: web::Data<AppState>,
    chat_info: web::Json<CreateChatRequest>,
) -> impl Responder {
    let new_chat_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();

    let is_group = chat_info.participants.len() > 2;
    let group_name = if is_group {
        // If user provided a group_name, use it; else "Unnamed Group"
        match &chat_info.group_name {
            Some(g) if !g.trim().is_empty() => g.clone(),
            _ => "Unnamed Group".to_string(),
        }
    } else {
        // For direct 1:1 chat, we might leave group_name as None
        String::new()
    };

    let new_chat = Chat {
        id_chat: new_chat_id.clone(),
        participants: chat_info.participants.clone(),
        is_group,
        group_name: if is_group { Some(group_name) } else { None },
        created_at: now,
        last_message_at: now,
    };

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    if let Err(e) = chats_collection.insert_one(&new_chat).await {
        return HttpResponse::InternalServerError().body(format!("Failed to create chat: {}", e));
    }

    // Possibly create an initial message if desired:
    // For example, we do chat_info.message = "Chat initiated."
    // If you do not want to store that, skip.
    // If you do want to store that:
    // let initial_msg = ...
    // chat_server.send(...) etc.

    // Return an HttpResponse directly (no `Ok(...)`)
    HttpResponse::Ok().json(&new_chat)
}

// ----------------------------------------------------------------------
// GET /chats/search/{user_id}?q=someQuery => example search
// ----------------------------------------------------------------------
pub async fn search_chats(
    data: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let user_id_str = path.into_inner();
    let _search_str = query.get("q").unwrap_or(&"".to_string()).to_lowercase();

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
        match chat_res {
            Ok(chat_doc) => result_chats.push(chat_doc),
            Err(_) => {}
        }
    }
    HttpResponse::Ok().json(result_chats)
}

// ----------------------------------------------------------------------
// DELETE /chats/{chat_id} => remove chat if user is participant
// ----------------------------------------------------------------------
pub async fn delete_chat(
    data: web::Data<AppState>,
    chat_id_path: web::Path<String>,
    req: HttpRequest,
) -> impl Responder {
    let chat_id_str = chat_id_path.into_inner();

    // Must have user_id from auth
    let user_id_opt = req.extensions().get::<String>().cloned();
    if user_id_opt.is_none() {
        return HttpResponse::Unauthorized().body("Unauthorized");
    }
    let user_id = user_id_opt.unwrap();

    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "_id": &chat_id_str };

    let chat_doc = match chats_collection.find_one(filter.clone()).await {
        Ok(Some(c)) => c,
        Ok(None) => return HttpResponse::NotFound().body("Chat not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching chat: {}", e)),
    };

    // Ensure the user is a participant
    if !chat_doc.participants.iter().any(|p| p == &user_id) {
        return HttpResponse::Unauthorized().body("Not a participant in the chat");
    }

    match chats_collection.delete_one(filter).await {
        Ok(_) => {
            // Also remove all messages in this chat
            let messages_collection = data.mongodb.db.collection::<DBMessage>("messages");
            let _ = messages_collection.delete_many(doc! { "id_chat": &chat_id_str }).await;
            HttpResponse::Ok().body("Chat deleted successfully")
        },
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting chat: {}", e)),
    }
}

// ----------------------------------------------------------------------
// POST /messages/{chat_id} => create a new message
// ----------------------------------------------------------------------
pub async fn create_message(
    req: HttpRequest,
    data: web::Data<AppState>,
    chat_id_path: web::Path<String>,
    payload: web::Json<CreateMessagePayload>,
) -> impl Responder {
    let chat_id_str = chat_id_path.into_inner();

    // Confirm user is in chat doc
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    match chats_collection
        .find_one(doc! { "_id": &chat_id_str, "participants": &payload.sender_id })
        .await
    {
        Ok(Some(_)) => { /* user is a participant */ }
        _ => {
            return HttpResponse::BadRequest().body("You are not a participant in this chat");
        }
    }

    // Send actor message
    let create_msg = crate::chat_server::CreateMessage {
        user_id: payload.sender_id.clone(),
        chat_id: chat_id_str.clone(),
        content: payload.content.clone(),
        attachments: None,
    };

    let chat_server = data.chat_server.clone();
    match chat_server.send(create_msg).await {
        Ok(Ok(msg_response)) => HttpResponse::Ok().json(msg_response),
        Ok(Err(_)) => HttpResponse::InternalServerError().body("Failed to create message"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Actor mailbox error: {:?}", e)),
    }
}
