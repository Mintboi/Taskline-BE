// src/board.rs
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use futures_util::StreamExt;
use mongodb::bson::{doc, to_document};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;
use log::{error, info};

use crate::app_state::AppState;

/// The Board model, now with embedded participants.
#[derive(Debug, Serialize, Deserialize)]
pub struct Board {
    pub board_id: String,
    pub project_id: String,
    pub name: String,
    pub board_type: String,          // "kanban" or "agile"
    pub description: Option<String>,
    pub sprint_length: Option<i32>,  // only applies to "agile"
    pub created_at: chrono::DateTime<Utc>,
    pub created_by: String,
    pub participants: Vec<String>,   // ✅ new field
}

/// Request payload for creating/updating a Board
#[derive(Debug, Deserialize)]
pub struct CreateOrUpdateBoardRequest {
    pub name: String,
    pub description: Option<String>,
    pub board_type: String,
    pub sprint_length: Option<i32>,
}

/// Request payload for adding a user to a board
#[derive(Debug, Deserialize)]
pub struct AddUserToBoardRequest {
    pub user_id: String,
}

/// GET /teams/{team_id}/projects/{project_id}/boards
/// List all boards for a project.
pub async fn list_boards(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
) -> impl Responder {
    let (team_id, project_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // 1) Must be on the team
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    if user_teams
        .find_one(doc! { "team_id": &team_id, "user_id": &current_user })
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }

    // 2) Must be a project member OR a board participant
    let project_memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let is_proj_member = project_memberships
        .find_one(doc! { "project_id": &project_id, "user_id": &current_user })
        .await
        .ok()
        .flatten()
        .is_some();

    let boards_coll = data.mongodb.db.collection::<Board>("boards");
    if !is_proj_member {
        // if not in project, check board‐level participation
        if boards_coll
            .find_one(doc! { "project_id": &project_id, "participants": &current_user })
            .await
            .ok()
            .flatten()
            .is_none()
        {
            return HttpResponse::Unauthorized().body("Not a member of this project or board");
        }
    }

    // 3) Fetch and return boards
    let mut cursor = match boards_coll.find(doc! { "project_id": &project_id }).await {
        Ok(c) => c,
        Err(e) => {
            error!("Error finding boards: {}", e);
            return HttpResponse::InternalServerError().body("Error finding boards");
        }
    };

    let mut boards = Vec::new();
    while let Some(r) = cursor.next().await {
        match r {
            Ok(b) => boards.push(b),
            Err(e) => {
                error!("Cursor error: {}", e);
                return HttpResponse::InternalServerError().body("Error reading boards");
            }
        }
    }

    HttpResponse::Ok().json(boards)
}

/// POST /teams/{team_id}/projects/{project_id}/boards
/// Create a new board for a project.
pub async fn create_board(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
    payload: web::Json<CreateOrUpdateBoardRequest>,
) -> impl Responder {
    let (team_id, project_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // same team/project membership checks as above...

    // seed participants with creator
    let new_board = Board {
        board_id: Uuid::new_v4().to_string(),
        project_id,
        name: payload.name.clone(),
        board_type: payload.board_type.clone(),
        description: payload.description.clone(),
        sprint_length: payload.sprint_length,
        created_at: Utc::now(),
        created_by: current_user.clone(),
        participants: vec![current_user.clone()], // ✅ include creator
    };

    let boards_coll = data.mongodb.db.collection::<Board>("boards");
    match boards_coll.insert_one(&new_board).await {
        Ok(_) => {
            info!("Board created: {:?}", new_board.board_id);
            HttpResponse::Ok().json(new_board)
        },
        Err(e) => {
            error!("Error inserting board: {}", e);
            HttpResponse::InternalServerError().body("Error inserting board")
        }
    }
}

/// PUT /teams/{team_id}/projects/{project_id}/boards/{board_id}
/// Update an existing board’s metadata.
pub async fn update_board(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>,
    payload: web::Json<CreateOrUpdateBoardRequest>,
) -> impl Responder {
    let (team_id, project_id, board_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // membership checks...

    let boards_coll = data.mongodb.db.collection::<Board>("boards");
    let filter = doc! { "board_id": &board_id, "project_id": &project_id };

    let mut update_doc = doc! {
        "name": &payload.name,
        "board_type": &payload.board_type,
        "description": &payload.description,
    };
    let sprint_val = if payload.board_type.to_lowercase() == "agile" {
        payload.sprint_length
    } else {
        None
    };
    update_doc.insert("sprint_length", sprint_val);

    let update_op = doc! { "$set": update_doc };
    match boards_coll.update_one(filter, update_op).await {
        Ok(res) if res.matched_count == 1 => HttpResponse::Ok().body("Board updated"),
        Ok(_) => HttpResponse::NotFound().body("Board not found"),
        Err(e) => {
            error!("Error updating board: {}", e);
            HttpResponse::InternalServerError().body("Error updating board")
        }
    }
}

/// DELETE /teams/{team_id}/projects/{project_id}/boards/{board_id}
pub async fn delete_board(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>,
) -> impl Responder {
    let (team_id, project_id, board_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // membership checks...

    let boards_coll = data.mongodb.db.collection::<Board>("boards");
    let filter = doc! { "board_id": &board_id, "project_id": &project_id };
    match boards_coll.delete_one(filter).await {
        Ok(res) if res.deleted_count == 1 => HttpResponse::Ok().body("Board deleted"),
        Ok(_) => HttpResponse::NotFound().body("Board not found or already deleted"),
        Err(e) => {
            error!("Error deleting board: {}", e);
            HttpResponse::InternalServerError().body("Error deleting board")
        }
    }
}

/// POST /teams/{team_id}/projects/{project_id}/boards/{board_id}/members
/// Add an existing project user to a board.
pub async fn add_user_to_board(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>,
    payload: web::Json<AddUserToBoardRequest>,
) -> impl Responder {
    let (team_id, project_id, board_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // 1) Caller must be a team member.
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let caller_filter = doc! { "team_id": &team_id, "user_id": &current_user };
    if user_teams.find_one(caller_filter).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }

    // 2) Target user must also be a team member.
    let target_filter = doc! { "team_id": &team_id, "user_id": &payload.user_id };
    if user_teams.find_one(target_filter).await.ok().flatten().is_none() {
        return HttpResponse::BadRequest().body("User is not a member of this team");
    }

    // 3) Add to the board’s participants array
    let boards_coll = data.mongodb.db.collection::<Board>("boards");
    let filter = doc! { "board_id": &board_id, "project_id": &project_id };
    let update = doc! {
        "$addToSet": { "participants": &payload.user_id }
    };
    match boards_coll.update_one(filter, update).await {
        Ok(res) if res.matched_count == 1 => {
            info!("User {} added to board {}", payload.user_id, board_id);
            HttpResponse::Ok().body("User added to board")
        }
        Ok(_) => HttpResponse::NotFound().body("Board not found"),
        Err(e) => {
            error!("Error adding user to board: {}", e);
            HttpResponse::InternalServerError().body("Error adding user to board")
        }
    }
}
