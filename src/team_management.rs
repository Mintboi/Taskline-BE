// team_management.rs

use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use futures_util::StreamExt;
use mongodb::bson::{doc, to_document};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;
use log::{debug, error, info};

use crate::app_state::AppState;
use crate::models::Chat;
// ─── DATA STRUCTURES ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Team {
    pub team_id: String,
    pub name: String,
    pub owner_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserTeam {
    pub user_id: String,
    pub team_id: String,
    pub role: String, // "admin" or "member"
    pub joined_at: chrono::DateTime<Utc>,
}

// For convenience, a shorthand alias for team members.
pub type TeamMember = UserTeam;

// ─── REQUEST PAYLOADS ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct InviteRequest {
    pub team_id: String,
    pub invitee_id: String,
}

// New payload for updating a team.
#[derive(Debug, Deserialize)]
pub struct UpdateTeamRequest {
    pub name: String,
}

// New payload for removing a team member.
#[derive(Debug, Deserialize)]
pub struct RemoveTeamMemberRequest {
    pub team_id: String,
    pub user_id: String,
}

// ─── EXISTING ENDPOINTS ─────────────────────────────────────────────────────────

// GET /user_teams/{user_id}
// Returns a list of teams (from the join table) the authenticated user belongs to.
// Validates that the `user_id` in the path matches the authenticated user.
pub async fn get_user_teams(
    req: HttpRequest,
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    // Extract the authenticated user ID from request extensions.
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Ensure the caller is only requesting their own teams.
    if current_user != *user_id {
        return HttpResponse::Unauthorized().body("Cannot access other user's teams");
    }

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let filter = doc! { "user_id": &*user_id };

    let mut cursor = match user_teams_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            error!("Error fetching teams: {}", err);
            return HttpResponse::InternalServerError()
                .body(format!("Error fetching teams: {}", err));
        }
    };

    let mut user_teams: Vec<UserTeam> = Vec::new();
    while let Some(team_result) = cursor.next().await {
        match team_result {
            Ok(user_team) => user_teams.push(user_team),
            Err(err) => {
                error!("Error iterating teams: {}", err);
                return HttpResponse::InternalServerError()
                    .body(format!("Error iterating teams: {}", err));
            }
        }
    }

    HttpResponse::Ok().json(user_teams)
}

// GET /chats/{user_id}
// Returns the list of chats the user participates in.
pub async fn get_user_chats(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "participants": &*user_id };

    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            error!("Error fetching chats: {}", err);
            return HttpResponse::InternalServerError()
                .body(format!("Error fetching chats: {}", err));
        }
    };

    let mut chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        match chat_res {
            Ok(chat) => chats.push(chat),
            Err(err) => {
                error!("Error iterating over chats: {}", err);
                return HttpResponse::InternalServerError()
                    .body(format!("Error iterating over chats: {}", err));
            }
        }
    }

    HttpResponse::Ok().json(chats)
}

// (The Chat and CreateChatRequest structs are assumed to be defined elsewhere.)

// POST /create_team
// Creates a new team. The authenticated user becomes the team owner and is
// automatically added as an admin in the "user_teams" join table.
pub async fn create_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_info: web::Json<CreateTeamRequest>,
) -> impl Responder {
    debug!("create_team endpoint called with payload: {:?}", team_info);
    // Extract the authenticated user's ID.
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        error!("Unauthorized: No authenticated user found in request extensions");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");

    let new_team_id = Uuid::new_v4().to_string();
    let new_team = Team {
        team_id: new_team_id.clone(),
        name: team_info.name.clone(),
        owner_id: current_user.clone(),
        created_at: Utc::now(),
    };

    debug!("Creating team with new_team: {:?}", new_team);
    // Insert the new team document.
    match teams_collection.insert_one(&new_team).await {
        Ok(_) => {
            // Record the creator's membership as an admin.
            let user_team = UserTeam {
                user_id: current_user,
                team_id: new_team_id.clone(),
                role: "admin".to_string(),
                joined_at: Utc::now(),
            };

            debug!("Inserting user_team membership: {:?}", user_team);
            match user_teams_collection.insert_one(&user_team).await {
                Ok(_) => {
                    info!("Team created successfully: {:?}", new_team);
                    HttpResponse::Ok().json(new_team)
                },
                Err(err) => {
                    error!("Error assigning team admin: {}", err);
                    HttpResponse::InternalServerError()
                        .body(format!("Error assigning team admin: {}", err))
                }
            }
        }
        Err(err) => {
            error!("Error creating team: {}", err);
            HttpResponse::InternalServerError()
                .body(format!("Error creating team: {}", err))
        }
    }
}

// POST /invite
// Invites a user to join a team. Only an admin in the team may perform this action.
pub async fn invite_user(
    req: HttpRequest,
    data: web::Data<AppState>,
    invite_info: web::Json<InviteRequest>,
) -> impl Responder {
    // Extract the authenticated user's ID.
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        error!("Unauthorized: No authenticated user found in invite_user");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");

    // Verify that the current user is an admin in the specified team.
    let admin_filter = doc! {
        "team_id": &invite_info.team_id,
        "user_id": &current_user,
        "role": "admin"
    };

    match user_teams_collection.find_one(admin_filter).await {
        Ok(Some(_)) => {
            // Optionally, check that the invitee is not already a member.
            let member_filter = doc! {
                "team_id": &invite_info.team_id,
                "user_id": &invite_info.invitee_id,
            };

            match user_teams_collection.find_one(member_filter).await {
                Ok(Some(_)) => {
                    return HttpResponse::BadRequest()
                        .body("User is already a member of the team");
                }
                Ok(None) => {
                    // Add the invitee as a team member.
                    let new_member = UserTeam {
                        user_id: invite_info.invitee_id.clone(),
                        team_id: invite_info.team_id.clone(),
                        role: "member".to_string(),
                        joined_at: Utc::now(),
                    };

                    match user_teams_collection.insert_one(&new_member).await {
                        Ok(_) => {
                            info!("User {} invited to team {}", invite_info.invitee_id, invite_info.team_id);
                            HttpResponse::Ok().body("User invited successfully")
                        },
                        Err(err) => {
                            error!("Error inviting user: {}", err);
                            HttpResponse::InternalServerError()
                                .body(format!("Error inviting user: {}", err))
                        },
                    }
                }
                Err(err) => {
                    error!("Error checking membership: {}", err);
                    HttpResponse::InternalServerError()
                        .body(format!("Error checking membership: {}", err))
                }
            }
        }
        Ok(None) => HttpResponse::Unauthorized().body("Only team admins can invite users"),
        Err(err) => HttpResponse::InternalServerError()
            .body(format!("Error checking admin status: {}", err)),
    }
}

// GET /user_teams/{team_id}/members
// Returns the list of team members for a given team.
// Only a user who belongs to the team can view its members.
pub async fn get_team_members(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    // Extract the authenticated user's ID.
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");

    // Verify that the current user is a member of the team.
    let membership_filter = doc! {
        "team_id": &*team_id,
        "user_id": &current_user,
    };

    match user_teams_collection.find_one(membership_filter).await {
        Ok(Some(_)) => {
            // Fetch all team members.
            let filter = doc! { "team_id": &*team_id };
            let mut cursor = match user_teams_collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(err) => {
                    return HttpResponse::InternalServerError()
                        .body(format!("Error fetching team members: {}", err))
                }
            };

            let mut members: Vec<UserTeam> = Vec::new();
            while let Some(member_res) = cursor.next().await {
                match member_res {
                    Ok(member) => members.push(member),
                    Err(err) => {
                        return HttpResponse::InternalServerError()
                            .body(format!("Error iterating team members: {}", err))
                    }
                }
            }

            HttpResponse::Ok().json(members)
        }
        Ok(None) => HttpResponse::Unauthorized().body("You are not a member of this team"),
        Err(err) => HttpResponse::InternalServerError()
            .body(format!("Error checking membership: {}", err)),
    }
}

// New Endpoint: GET /teams/{team_id}
// Retrieves the details of a team. Only members can view team details.
pub async fn get_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify membership
    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let membership_filter = doc! { "team_id": &*team_id, "user_id": &current_user };
    match user_teams_collection.find_one(membership_filter).await {
        Ok(Some(_)) => {},
        Ok(None) => return HttpResponse::Unauthorized().body("Not a member of the team"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error checking membership: {}", e)),
    }

    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let filter = doc! { "team_id": &*team_id };
    match teams_collection.find_one(filter).await {
        Ok(Some(team)) => HttpResponse::Ok().json(team),
        Ok(None) => HttpResponse::NotFound().body("Team not found"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error fetching team: {}", e)),
    }
}

// New Endpoint: PUT /teams/{team_id}
// Allows the team owner to update team details (currently only the team name).
pub async fn update_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
    team_info: web::Json<UpdateTeamRequest>,
) -> impl Responder {
    let team_id = team_id.into_inner();
    let current_user = if let Some(id) = req.extensions().get::<String>() { id.clone() } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    // Check if team exists and if current_user is owner
    let filter = doc! { "team_id": &team_id };
    let team = match teams_collection.find_one(filter.clone()).await {
        Ok(Some(team)) => team,
        Ok(None) => return HttpResponse::NotFound().body("Team not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching team: {}", e)),
    };
    if team.owner_id != current_user {
        return HttpResponse::Unauthorized().body("Only team owner can update team");
    }
    // Update the team name
    let update = doc! { "$set": { "name": &team_info.name } };
    match teams_collection.update_one(filter, update).await {
        Ok(_) => HttpResponse::Ok().body("Team updated successfully"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating team: {}", e)),
    }
}

// New Endpoint: DELETE /teams/{team_id}
// Allows the team owner to delete a team and remove all its membership records.
pub async fn delete_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let team_id = team_id.into_inner();
    let current_user = if let Some(id) = req.extensions().get::<String>() { id.clone() } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let filter = doc! { "team_id": &team_id };
    let team = match teams_collection.find_one(filter.clone()).await {
        Ok(Some(team)) => team,
        Ok(None) => return HttpResponse::NotFound().body("Team not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching team: {}", e)),
    };
    if team.owner_id != current_user {
        return HttpResponse::Unauthorized().body("Only team owner can delete team");
    }
    // Delete team document
    match teams_collection.delete_one(filter.clone()).await {
        Ok(_) => {
            // Also delete associated user_teams entries
            let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
            let membership_filter = doc! { "team_id": &team_id };
            let _ = user_teams_collection.delete_many(membership_filter).await;
            HttpResponse::Ok().body("Team deleted successfully")
        },
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting team: {}", e)),
    }
}

// New Endpoint: DELETE /teams/members
// Allows a team admin to remove a team member.
pub async fn remove_team_member(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RemoveTeamMemberRequest>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() { id.clone() } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    // Verify that the current user is an admin in the team
    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let admin_filter = doc! {
         "team_id": &info.team_id,
         "user_id": &current_user,
         "role": "admin"
    };
    match user_teams_collection.find_one(admin_filter).await {
        Ok(Some(_)) => {},
        Ok(None) => return HttpResponse::Unauthorized().body("Only team admins can remove members"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error verifying admin status: {}", e)),
    }
    // Remove the member specified by info.user_id from the team
    let member_filter = doc! {
         "team_id": &info.team_id,
         "user_id": &info.user_id,
    };
    match user_teams_collection.delete_one(member_filter).await {
        Ok(result) => {
            if result.deleted_count == 1 {
                HttpResponse::Ok().body("Member removed successfully")
            } else {
                HttpResponse::NotFound().body("Member not found in team")
            }
        },
        Err(e) => HttpResponse::InternalServerError().body(format!("Error removing member: {}", e)),
    }
}
