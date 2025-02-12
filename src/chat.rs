// File: chat.rs

use actix_web::{web, HttpResponse, Responder, HttpRequest};
use futures_util::StreamExt;
use mongodb::bson::{doc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;
use crate::app_state::AppState;

#[derive(Serialize, Deserialize, Clone)]
pub struct Team {
    pub team_id: String,
    pub name: String,
    pub owner_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserTeam {
    pub user_id: String,
    pub team_id: String,
    pub role: String,
    pub joined_at: chrono::DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct InviteRequest {
    pub team_id: String,
    pub invitee_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Chat {
    pub id_chat: Uuid,
    pub participants: Vec<Uuid>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub last_message_at: chrono::DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct CreateChatRequest {
    pub team_id: String,
    pub participants: Vec<Uuid>,
    pub message: String,
}

pub async fn get_user_teams(
    req: HttpRequest,
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let filter = doc! { "user_id": &*user_id };
    let mut cursor = match user_teams_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => return HttpResponse::InternalServerError().body(format!("Error fetching teams: {}", err))
    };
    let mut user_teams: Vec<UserTeam> = Vec::new();
    while let Some(team_result) = cursor.next().await {
        match team_result {
            Ok(user_team) => user_teams.push(user_team),
            Err(err) => return HttpResponse::InternalServerError().body(format!("Error iterating teams: {}", err))
        }
    }
    HttpResponse::Ok().json(user_teams)
}

pub async fn get_user_chats(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "participants": &*user_id };
    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => return HttpResponse::InternalServerError().body(format!("Error fetching chats: {}", err))
    };
    let mut chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        match chat_res {
            Ok(chat) => chats.push(chat),
            Err(err) => return HttpResponse::InternalServerError().body(format!("Error iterating over chats: {}", err))
        }
    }
    HttpResponse::Ok().json(chats)
}

pub async fn create_chat(
    data: web::Data<AppState>,
    chat_info: web::Json<CreateChatRequest>,
) -> impl Responder {
    let chat_collection = data.mongodb.db.collection::<Chat>("chats");
    let new_chat = Chat {
        id_chat: Uuid::new_v4(),
        participants: chat_info.participants.clone(),
        is_group: chat_info.participants.len() > 2,
        group_name: None,
        created_at: Utc::now(),
        last_message_at: Utc::now(),
    };
    match chat_collection.insert_one(&new_chat).await {
        Ok(_) => HttpResponse::Ok().json(new_chat),
        Err(err) => HttpResponse::InternalServerError().body(format!("Error creating chat: {}", err)),
    }
}

pub async fn get_team_members(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let filter = doc! { "team_id": &*team_id };
    match teams_collection.find_one(filter).await {
        Ok(Some(team)) => HttpResponse::Ok().json(team),
        Ok(None) => HttpResponse::NotFound().body("Team not found"),
        Err(err) => HttpResponse::InternalServerError().body(format!("Error fetching team members: {}", err)),
    }
}
