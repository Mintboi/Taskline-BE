use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use futures_util::StreamExt;
use mongodb::bson::{doc, to_document, DateTime as BsonDateTime, oid::ObjectId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::Utc;
use log::{debug, error, info};

use crate::app_state::AppState;
use crate::models::Chat;

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
    pub user_id: String, // stored in user_teams as the hex string of `_id`
    pub team_id: String,
    pub role: String,   // "admin" or "member"
    pub joined_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamInvitation {
    pub invitation_id: String,
    pub team_id: String,
    pub invitee_id: String,   // stored as hex string or username if not yet accepted
    pub inviter_id: String,
    pub status: String,       // "pending", "accepted", or "declined"
    pub sent_at: chrono::DateTime<Utc>,
    pub responded_at: Option<chrono::DateTime<Utc>>,
}

pub type TeamMember = UserTeam;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    #[serde(rename = "_id")]
    pub id: ObjectId,          // real field name is "_id"
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
    let chats_collection = data.mongodb.db.collection::<Chat>("chats");
    let filter = doc! { "participants": &*user_id };
    let mut cursor = match chats_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            error!("Error fetching chats: {}", err);
            return HttpResponse::InternalServerError().body(format!("Error fetching chats: {}", err));
        }
    };

    let mut chats = Vec::new();
    while let Some(chat_res) = cursor.next().await {
        match chat_res {
            Ok(chat) => chats.push(chat),
            Err(err) => {
                error!("Error iterating over chats: {}", err);
                return HttpResponse::InternalServerError().body(format!("Error iterating over chats: {}", err));
            }
        }
    }

    HttpResponse::Ok().json(chats)
}

pub async fn create_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_info: web::Json<CreateTeamRequest>,
) -> impl Responder {
    debug!("create_team endpoint called with payload: {:?}", team_info);

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
        description: Some(team_info.description.clone()),
        created_at: Utc::now(),
    };

    debug!("Creating team with new_team: {:?}", new_team);
    match teams_collection.insert_one(&new_team).await {
        Ok(_) => {
            let user_team = UserTeam {
                user_id: current_user.clone(),
                team_id: new_team_id.clone(),
                role: "admin".to_string(),
                joined_at: Utc::now(),
            };

            debug!("Inserting user_team membership: {:?}", user_team);
            match user_teams_collection.insert_one(&user_team).await {
                Ok(_) => {
                    let users_collection = data.mongodb.db.collection::<mongodb::bson::Document>("users");
                    if let Ok(oid) = ObjectId::parse_str(&current_user) {
                        let user_filter = doc! { "_id": oid };
                        let user_update = doc! { "$set": { "team_id": &new_team_id } };
                        let _ = users_collection.update_one(user_filter, user_update).await;
                    }

                    info!("Team created successfully: {:?}", new_team);
                    HttpResponse::Ok().json(new_team)
                },
                Err(err) => {
                    error!("Error assigning team admin: {}", err);
                    HttpResponse::InternalServerError().body(format!("Error assigning team admin: {}", err))
                }
            }
        },
        Err(err) => {
            error!("Error creating team: {}", err);
            HttpResponse::InternalServerError().body(format!("Error creating team: {}", err))
        }
    }
}

pub async fn invite_user(
    req: HttpRequest,
    data: web::Data<AppState>,
    invite_info: web::Json<InviteRequest>,
) -> impl Responder {
    let team_id = req.match_info().get("team_id").unwrap_or("").to_string();

    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        error!("Unauthorized: No authenticated user found in invite_user");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");

    let admin_filter = doc! {
        "team_id": &team_id,
        "user_id": &current_user,
        "role": "admin"
    };

    match user_teams_collection.find_one(admin_filter).await {
        Ok(Some(_)) => {
            let member_filter = doc! {
                "team_id": &team_id,
                "user_id": &invite_info.invitee_id,
            };
            if let Ok(Some(_)) = user_teams_collection.find_one(member_filter).await {
                return HttpResponse::BadRequest().body("User is already a member of the team");
            }

            let invitation_filter = doc! {
                "team_id": &team_id,
                "invitee_id": &invite_info.invitee_id,
                "status": "pending"
            };
            if let Ok(Some(_)) = invitations_collection.find_one(invitation_filter).await {
                return HttpResponse::BadRequest().body("An invitation is already pending for this user");
            }

            let new_invitation = TeamInvitation {
                invitation_id: Uuid::new_v4().to_string(),
                team_id: team_id.clone(),
                invitee_id: invite_info.invitee_id.clone(),
                inviter_id: current_user.clone(),
                status: "pending".to_string(),
                sent_at: Utc::now(),
                responded_at: None,
            };

            match invitations_collection.insert_one(new_invitation).await {
                Ok(_) => {
                    info!("User {} invited to team {}", invite_info.invitee_id, team_id);
                    HttpResponse::Ok().body("Invitation sent successfully")
                },
                Err(err) => {
                    error!("Error inviting user: {}", err);
                    HttpResponse::InternalServerError().body(format!("Error inviting user: {}", err))
                }
            }
        }
        Ok(None) => HttpResponse::Unauthorized().body("Only team admins can invite users"),
        Err(err) => HttpResponse::InternalServerError().body(format!("Error checking admin status: {}", err)),
    }
}

pub async fn get_team_members(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let membership_filter = doc! {
        "team_id": &*team_id,
        "user_id": &current_user,
    };

    match user_teams_collection.find_one(membership_filter).await {
        Ok(Some(_)) => {
            let mut combined_members: Vec<TeamMemberInfo> = Vec::new();

            // Fetch accepted members (from user_teams)
            let filter = doc! { "team_id": &*team_id };
            let mut cursor = match user_teams_collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(err) => {
                    return HttpResponse::InternalServerError().body(format!("Error fetching team members: {}", err))
                }
            };
            let users_collection = data.mongodb.db.collection::<User>("users");

            while let Some(member_res) = cursor.next().await {
                if let Ok(member) = member_res {
                    if let Ok(member_oid) = ObjectId::parse_str(&member.user_id) {
                        let user_filter = doc! { "_id": member_oid };
                        if let Ok(Some(user_doc)) = users_collection.find_one(user_filter).await {
                            combined_members.push(TeamMemberInfo {
                                user_id: member.user_id.clone(),
                                email: user_doc.email.clone(),
                                username: user_doc.username.clone(),
                                status: "accepted".to_string(),
                                invitation_id: None,
                            });
                        } else {
                            combined_members.push(TeamMemberInfo {
                                user_id: member.user_id.clone(),
                                email: format!("(unknown user_id {})", member.user_id),
                                username: None,
                                status: "accepted".to_string(),
                                invitation_id: None,
                            });
                        }
                    } else {
                        // Non-OID user_id fallback
                        combined_members.push(TeamMemberInfo {
                            user_id: member.user_id.clone(),
                            email: format!("(unknown user_id {})", member.user_id),
                            username: None,
                            status: "accepted".to_string(),
                            invitation_id: None,
                        });
                    }
                }
            }

            // Fetch pending invites (from team_invitations)
            let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
            let inv_filter = doc! {
                "team_id": &*team_id,
                "status": "pending"
            };
            let mut inv_cursor = match invitations_collection.find(inv_filter).await {
                Ok(cursor) => cursor,
                Err(err) => {
                    return HttpResponse::InternalServerError().body(format!("Error fetching invitations: {}", err))
                }
            };

            while let Some(inv_res) = inv_cursor.next().await {
                if let Ok(inv) = inv_res {
                    // Try to parse invitee_id as ObjectId
                    if let Ok(_inv_oid) = ObjectId::parse_str(&inv.invitee_id) {
                        let user_filter = doc! { "_id": ObjectId::parse_str(&inv.invitee_id).unwrap() };
                        if let Ok(Some(user_doc)) = users_collection.find_one(user_filter).await {
                            combined_members.push(TeamMemberInfo {
                                user_id: user_doc.id.to_hex(),
                                email: user_doc.email.clone(),
                                username: user_doc.username.clone(),
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        } else {
                            combined_members.push(TeamMemberInfo {
                                user_id: inv.invitee_id.clone(),
                                email: inv.invitee_id.clone(),
                                username: None,
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        }
                    } else {
                        // If it's not an ObjectId, treat invitee_id as username
                        let trimmed = inv.invitee_id.trim();
                        let user_filter = doc! { "username": trimmed };

                        if let Ok(Some(user_doc)) = users_collection.find_one(user_filter).await {
                            combined_members.push(TeamMemberInfo {
                                user_id: user_doc.id.to_hex(),
                                email: user_doc.email.clone(),
                                username: user_doc.username.clone(),
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        } else {
                            // --------------------------------------------------------------
                            // FIX: Instead of storing the raw username in `user_id` field,
                            //      we push it to `username`, and leave `user_id` as "" (or any placeholder).
                            // --------------------------------------------------------------
                            combined_members.push(TeamMemberInfo {
                                user_id: "".to_string(),                // <-- changed
                                email: "".to_string(),                  // <-- changed (or keep invitee_id if you prefer)
                                username: Some(inv.invitee_id.clone()), // <-- changed (now uses invitee_id as username)
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        }
                    }
                }
            }

            HttpResponse::Ok().json(combined_members)
        }
        Ok(None) => HttpResponse::Unauthorized().body("You are not a member of this team"),
        Err(err) => HttpResponse::InternalServerError().body(format!("Error checking membership: {}", err)),
    }
}

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

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let membership_filter = doc! { "team_id": &*team_id, "user_id": &current_user };

    match user_teams_collection.find_one(membership_filter).await {
        Ok(Some(_)) => {}
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

pub async fn update_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
    team_info: web::Json<UpdateTeamRequest>,
) -> impl Responder {
    let team_id = team_id.into_inner();
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
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
        return HttpResponse::Unauthorized().body("Only team owner can update team");
    }

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let mut update_doc = doc! { "$set": { "name": &team_info.name } };

    if let Some(ref new_owner) = team_info.new_owner_id {
        if new_owner != &current_user {
            let membership_filter = doc! { "team_id": &team_id, "user_id": new_owner };
            match user_teams_collection.find_one(membership_filter).await {
                Ok(Some(_)) => {
                    update_doc
                        .get_document_mut("$set")
                        .unwrap()
                        .insert("owner_id", new_owner);
                }
                _ => {
                    return HttpResponse::BadRequest().body("New owner must be a member of the team")
                }
            }
        }
    }

    match teams_collection.update_one(filter, update_doc).await {
        Ok(_) => HttpResponse::Ok().body("Team updated successfully"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating team: {}", e)),
    }
}

pub async fn delete_team(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let team_id = team_id.into_inner();
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
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

    match teams_collection.delete_one(filter.clone()).await {
        Ok(_) => {
            let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
            let membership_filter = doc! { "team_id": &team_id };
            let _ = user_teams_collection.delete_many(membership_filter).await;

            HttpResponse::Ok().body("Team deleted successfully")
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting team: {}", e)),
    }
}

pub async fn remove_team_member(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RemoveTeamMemberRequest>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

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
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error removing member: {}", e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct AcceptDeclinePayload {
    pub invitation_id: String,
}

pub async fn accept_invitation(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RespondInvitationRequest>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");

    let filter = doc! { "invitation_id": &info.invitation_id };
    let invitation = match invitations_collection.find_one(filter.clone()).await {
        Ok(Some(inv)) => inv,
        Ok(None) => return HttpResponse::NotFound().body("Invitation not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching invitation: {}", e)),
    };

    if invitation.invitee_id != current_user {
        return HttpResponse::Unauthorized().body("You are not the invitee for this invitation");
    }
    if invitation.status != "pending" {
        return HttpResponse::BadRequest().body("Invitation is not pending");
    }

    let update = doc! {
        "$set": {
            "status": "accepted",
            "responded_at": BsonDateTime::from_millis(Utc::now().timestamp_millis())
        }
    };

    if let Err(e) = invitations_collection.update_one(filter.clone(), update).await {
        return HttpResponse::InternalServerError().body(format!("Error updating invitation: {}", e));
    }

    let membership_filter = doc! {
        "team_id": &invitation.team_id,
        "user_id": &current_user,
    };
    if let Ok(Some(_)) = user_teams_collection.find_one(membership_filter).await {
        return HttpResponse::BadRequest().body("You are already a member of this team");
    }

    let new_membership = UserTeam {
        user_id: current_user,
        team_id: invitation.team_id,
        role: "member".to_string(),
        joined_at: Utc::now(),
    };

    match user_teams_collection.insert_one(new_membership).await {
        Ok(_) => HttpResponse::Ok().body("Invitation accepted and team membership added"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error adding membership: {}", e)),
    }
}

pub async fn decline_invitation(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<RespondInvitationRequest>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
    let filter = doc! { "invitation_id": &info.invitation_id };

    let invitation = match invitations_collection.find_one(filter.clone()).await {
        Ok(Some(inv)) => inv,
        Ok(None) => return HttpResponse::NotFound().body("Invitation not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching invitation: {}", e)),
    };

    if invitation.invitee_id != current_user {
        return HttpResponse::Unauthorized().body("You are not the invitee for this invitation");
    }
    if invitation.status != "pending" {
        return HttpResponse::BadRequest().body("Invitation is not pending");
    }

    let update = doc! {
        "$set": {
            "status": "declined",
            "responded_at": BsonDateTime::from_millis(Utc::now().timestamp_millis())
        }
    };

    match invitations_collection.update_one(filter, update).await {
        Ok(_) => HttpResponse::Ok().body("Invitation declined"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating invitation: {}", e)),
    }
}

pub async fn delete_invitations(
    req: HttpRequest,
    data: web::Data<AppState>,
    info: web::Json<DeleteInvitationsRequest>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let user_teams_collection = data.mongodb.db.collection::<UserTeam>("user_teams");
    let admin_filter = doc! {
        "team_id": &info.team_id,
        "user_id": &current_user,
        "role": "admin"
    };

    match user_teams_collection.find_one(admin_filter).await {
        Ok(Some(_)) => {
            let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
            let filter = doc! {
                "team_id": &info.team_id,
                "invitation_id": { "$in": info.invitation_ids.iter().map(|s| s.to_owned()).collect::<Vec<_>>() }
            };

            match invitations_collection.delete_many(filter).await {
                Ok(delete_result) => {
                    let count = delete_result.deleted_count;
                    HttpResponse::Ok().body(format!("Deleted {} invitation(s)", count))
                },
                Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting invitations: {}", e))
            }
        }
        Ok(None) => HttpResponse::Unauthorized().body("Only team admins can delete invitations"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error verifying admin status: {}", e)),
    }
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
