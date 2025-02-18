use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use futures_util::StreamExt;
use mongodb::bson::{doc, to_document, DateTime as BsonDateTime, oid::ObjectId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;
use log::{debug, error, info};

use crate::app_state::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Team {
    pub team_id: String,
    pub name: String,
    pub owner_id: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserTeam {
    // stored in user_teams as the hex string of `_id`
    pub user_id: String,
    pub team_id: String,
    pub role: String,
    pub joined_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamInvitation {
    pub invitation_id: String,
    pub team_id: String,
    pub invitee_id: String,
    pub inviter_id: String,
    pub status: String,
    pub sent_at: chrono::DateTime<Utc>,
    pub responded_at: Option<chrono::DateTime<Utc>>,
}

pub type TeamMember = UserTeam;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub username: Option<String>,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamMemberInfo {
    pub user_id: String,
    pub email: String,
    pub username: Option<String>,
    pub status: String,
    pub invitation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct InviteRequest {
    pub invitee_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RespondInvitationRequest {
    pub invitation_id: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTeamRequest {
    pub name: String,
    pub new_owner_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveTeamMemberRequest {
    pub team_id: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteInvitationsRequest {
    pub team_id: String,
    pub invitation_ids: Vec<String>,
}

pub async fn get_user_teams(
    req: HttpRequest,
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    if current_user != *user_id {
        return HttpResponse::Unauthorized().body("Cannot access other user's teams");
    }

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let filter = doc! { "user_id": &*user_id };

    let mut cursor = match user_teams_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            error!("Error fetching teams: {}", err);
            return HttpResponse::InternalServerError().body(format!("Error fetching teams: {}", err));
        }
    };

    let mut user_teams: Vec<UserTeam> = Vec::new();
    while let Some(team_result) = cursor.next().await {
        match team_result {
            Ok(user_team) => user_teams.push(user_team),
            Err(err) => {
                error!("Error iterating teams: {}", err);
                return HttpResponse::InternalServerError().body(format!("Error iterating teams: {}", err));
            }
        }
    }

    HttpResponse::Ok().json(user_teams)
}

pub async fn get_user_chats(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn create_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_info: web::Json<CreateTeamRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn invite_user(
    req: HttpRequest,
    data: web::Data<AppState>,
    invite_info: web::Json<InviteRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn get_team_members(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn get_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn update_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
    team_info: web::Json<UpdateTeamRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn delete_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn remove_team_member(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RemoveTeamMemberRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn accept_invitation(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RespondInvitationRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn decline_invitation(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RespondInvitationRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

pub async fn delete_invitations(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<DeleteInvitationsRequest>,
) -> impl Responder {
    // Implementation omitted for brevity (same as before)
    HttpResponse::Ok().finish()
}

#[derive(Debug, Deserialize)]
pub struct FindUserQuery {
    pub query: String,
}

pub async fn find_user_email(
    query: web::Query<FindUserQuery>,
    data: web::Data<AppState>,
) -> impl Responder {
    let users_collection = data.mongodb.db.collection::<User>("users");
    let filter = doc! { "email": { "$regex": &query.query, "$options": "i" } };

    let mut cursor = match users_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => return HttpResponse::InternalServerError().body(format!("Error fetching users: {}", err)),
    };

    let mut users: Vec<User> = Vec::new();
    while let Some(result) = cursor.next().await {
        match result {
            Ok(user) => users.push(user),
            Err(err) => return HttpResponse::InternalServerError().body(format!("Error iterating users: {}", err)),
        }
    }

    HttpResponse::Ok().json(users)
}

// New endpoint: Get user information by id
pub async fn get_user_by_id(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> impl Responder {
    let users_collection = data.mongodb.db.collection::<User>("users");
    let id_str = path.into_inner();
    if let Ok(object_id) = ObjectId::parse_str(&id_str) {
        let filter = doc! { "_id": object_id };
        match users_collection.find_one(filter).await {
            Ok(Some(user)) => HttpResponse::Ok().json(user),
            Ok(None) => HttpResponse::NotFound().body("User not found"),
            Err(e) => HttpResponse::InternalServerError().body(format!("Error fetching user: {}", e)),
        }
    } else {
        HttpResponse::BadRequest().body("Invalid user id")
    }
}
