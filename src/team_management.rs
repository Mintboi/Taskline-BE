// File: team-management.rs
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
    // stored in user_teams as the hex string of `_id`
    pub user_id: String,
    pub team_id: String,
    pub role: String,   // "admin" or "member"
    pub joined_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TeamInvitation {
    pub invitation_id: String,
    pub team_id: String,
    // invitee_id is stored as a hex string if the user exists,
    // otherwise it might be left as the raw text (email/username) if no user was found.
    pub invitee_id: String,
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

/// Display object for invitations.
#[derive(Debug, Serialize, Deserialize)]
pub struct InvitationDisplay {
    pub invitation_id: String,
    pub team_id: String,
    pub team_name: String,
    pub inviter_username: String,
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

/// Retrieve pending invitations for a given user.
/// The endpoint verifies that the JWT user matches the requested user.
/// It then filters for invitations where invitee_id equals the user’s hex string.
pub async fn get_pending_invitations(
    req: HttpRequest,
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let current_user = if let Some(id) = req.extensions().get::<String>() {
        id.trim().to_string()
    } else {
        error!("No user found in request extensions for get_pending_invitations");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    let requested_user = user_id.trim().to_string();
    debug!("Token user id: '{}' | Requested user id: '{}'", current_user, requested_user);

    if current_user != requested_user {
        error!("User mismatch: token user id '{}' does not match requested user id '{}'", current_user, requested_user);
        return HttpResponse::Unauthorized().body("Cannot access other user's invitations");
    }

    let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
    let filter = doc! { "invitee_id": &requested_user, "status": "pending" };

    let mut cursor = match invitations_collection.find(filter).await {
        Ok(cursor) => cursor,
        Err(err) => {
            error!("Error fetching invitations: {}", err);
            return HttpResponse::InternalServerError().body(format!("Error fetching invitations: {}", err));
        }
    };

    let mut displays: Vec<InvitationDisplay> = Vec::new();
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let users_collection = data.mongodb.db.collection::<User>("users");

    while let Some(inv_result) = cursor.next().await {
        match inv_result {
            Ok(inv) => {
                // Look up team info.
                let team_filter = doc! { "team_id": &inv.team_id };
                let team_doc = teams_collection.find_one(team_filter).await.ok().flatten();
                let team_name = team_doc.map(|t| t.name).unwrap_or_else(|| "Unknown Team".into());

                // Look up inviter info.
                let inviter_obj_id = ObjectId::parse_str(&inv.inviter_id).ok();
                let inviter_username = if let Some(oid) = inviter_obj_id {
                    let inviter_filter = doc! { "_id": oid };
                    if let Ok(Some(inviter)) = users_collection.find_one(inviter_filter).await {
                        inviter.username.unwrap_or_else(|| "Unknown Inviter".into())
                    } else {
                        "Unknown Inviter".into()
                    }
                } else {
                    "Unknown Inviter".into()
                };

                displays.push(InvitationDisplay {
                    invitation_id: inv.invitation_id,
                    team_id: inv.team_id,
                    team_name,
                    inviter_username,
                });
            },
            Err(err) => {
                error!("Error iterating invitations: {}", err);
                return HttpResponse::InternalServerError().body(format!("Error iterating invitations: {}", err));
            }
        }
    }

    HttpResponse::Ok().json(displays)
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
                    HttpResponse::InternalServerError()
                        .body(format!("Error assigning team admin: {}", err))
                }
            }
        },
        Err(err) => {
            error!("Error creating team: {}", err);
            HttpResponse::InternalServerError()
                .body(format!("Error creating team: {}", err))
        }
    }
}

/// Updated invite_user endpoint using the "find_user_email" fix logic.
/// We now attempt to resolve the invitee_id: if it's not a valid ObjectId, we search by email then by username.
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
    let users_collection = data.mongodb.db.collection::<User>("users");

    // Ensure the requester is an admin of the team.
    let admin_filter = doc! {
        "team_id": &team_id,
        "user_id": &current_user,
        "role": "admin"
    };

    match user_teams_collection.find_one(admin_filter).await {
        Ok(Some(_)) => {
            // Resolve invitee_id: if it’s a valid ObjectId, use it;
            // otherwise, try to find a user by email then by username.
            let resolved_invitee_id = if ObjectId::parse_str(&invite_info.invitee_id).is_ok() {
                invite_info.invitee_id.clone()
            } else {
                let email_filter = doc! { "email": &invite_info.invitee_id };
                if let Ok(Some(user)) = users_collection.find_one(email_filter).await {
                    user.id.to_hex()
                } else {
                    let username_filter = doc! { "username": &invite_info.invitee_id };
                    if let Ok(Some(user)) = users_collection.find_one(username_filter).await {
                        user.id.to_hex()
                    } else {
                        return HttpResponse::BadRequest().body("User not found by email or username");
                    }
                }
            };

            let member_filter = doc! {
                "team_id": &team_id,
                "user_id": &resolved_invitee_id,
            };
            if let Ok(Some(_)) = user_teams_collection.find_one(member_filter).await {
                return HttpResponse::BadRequest().body("User is already a member of the team");
            }

            let invitation_filter = doc! {
                "team_id": &team_id,
                "invitee_id": &resolved_invitee_id,
                "status": "pending"
            };
            if let Ok(Some(_)) = invitations_collection.find_one(invitation_filter).await {
                return HttpResponse::BadRequest().body("An invitation is already pending for this user");
            }

            let new_invitation = TeamInvitation {
                invitation_id: Uuid::new_v4().to_string(),
                team_id: team_id.clone(),
                invitee_id: resolved_invitee_id.clone(),
                inviter_id: current_user.clone(),
                status: "pending".to_string(),
                sent_at: Utc::now(),
                responded_at: None,
            };

            match invitations_collection.insert_one(new_invitation).await {
                Ok(_) => {
                    info!("User {} invited to team {}", resolved_invitee_id, team_id);
                    HttpResponse::Ok().body("Invitation sent successfully")
                },
                Err(err) => {
                    error!("Error inviting user: {}", err);
                    HttpResponse::InternalServerError()
                        .body(format!("Error inviting user: {}", err))
                }
            }
        },
        Ok(None) => HttpResponse::Unauthorized().body("Only team admins can invite users"),
        Err(err) => HttpResponse::InternalServerError()
            .body(format!("Error checking admin status: {}", err)),
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

            // First: get all accepted members in user_teams
            let filter = doc! { "team_id": &*team_id };
            let mut cursor = match user_teams_collection.find(filter).await {
                Ok(cursor) => cursor,
                Err(err) => {
                    return HttpResponse::InternalServerError()
                        .body(format!("Error fetching team members: {}", err))
                }
            };

            let users_collection = data.mongodb.db.collection::<User>("users");

            while let Some(member_res) = cursor.next().await {
                if let Ok(member) = member_res {
                    if let Ok(member_oid) = ObjectId::parse_str(&member.user_id) {
                        // If user_id is a valid ObjectId, fetch the user
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
                            // OID didn't match any user; fallback
                            combined_members.push(TeamMemberInfo {
                                user_id: member.user_id.clone(),
                                email: member.user_id.clone(),
                                username: None,
                                status: "accepted".to_string(),
                                invitation_id: None,
                            });
                        }
                    } else {
                        // user_id is not a valid ObjectId
                        combined_members.push(TeamMemberInfo {
                            user_id: member.user_id.clone(),
                            email: member.user_id.clone(),
                            username: None,
                            status: "accepted".to_string(),
                            invitation_id: None,
                        });
                    }
                }
            }

            // Next: fetch all pending invitations
            let invitations_collection = data.mongodb.db.collection::<TeamInvitation>("team_invitations");
            let inv_filter = doc! {
                "team_id": &*team_id,
                "status": "pending"
            };
            let mut inv_cursor = match invitations_collection.find(inv_filter).await {
                Ok(cursor) => cursor,
                Err(err) => {
                    return HttpResponse::InternalServerError()
                        .body(format!("Error fetching invitations: {}", err))
                }
            };

            while let Some(inv_res) = inv_cursor.next().await {
                if let Ok(inv) = inv_res {
                    // 1) If invitee_id is a valid ObjectId, try to fetch that user
                    if let Ok(inv_oid) = ObjectId::parse_str(&inv.invitee_id) {
                        let user_filter = doc! { "_id": inv_oid };
                        if let Ok(Some(user_doc)) = users_collection.find_one(user_filter).await {
                            combined_members.push(TeamMemberInfo {
                                user_id: inv.invitee_id.clone(),
                                email: user_doc.email.clone(),
                                username: user_doc.username.clone(),
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        } else {
                            // Could not find user by that OID
                            combined_members.push(TeamMemberInfo {
                                user_id: "".to_string(),
                                email: inv.invitee_id.clone(),
                                username: Some(inv.invitee_id.clone()),
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        }
                    } else {
                        // 2) If not a valid ObjectId, attempt to find a user by email
                        let email_filter = doc! { "email": &inv.invitee_id };
                        if let Ok(Some(user_doc)) = users_collection.find_one(email_filter).await {
                            combined_members.push(TeamMemberInfo {
                                user_id: user_doc.id.to_hex(),
                                email: user_doc.email.clone(),
                                username: user_doc.username.clone(),
                                status: "pending".to_string(),
                                invitation_id: Some(inv.invitation_id.clone()),
                            });
                        } else {
                            // 3) If not found by email, try by username
                            let username_filter = doc! { "username": &inv.invitee_id };
                            if let Ok(Some(user_doc)) = users_collection.find_one(username_filter).await {
                                combined_members.push(TeamMemberInfo {
                                    user_id: user_doc.id.to_hex(),
                                    email: user_doc.email.clone(),
                                    username: user_doc.username.clone(),
                                    status: "pending".to_string(),
                                    invitation_id: Some(inv.invitation_id.clone()),
                                });
                            } else {
                                // 4) Fallback: store the raw invitee_id
                                combined_members.push(TeamMemberInfo {
                                    user_id: "".to_string(),
                                    email: inv.invitee_id.clone(),
                                    username: Some(inv.invitee_id.clone()),
                                    status: "pending".to_string(),
                                    invitation_id: Some(inv.invitation_id.clone()),
                                });
                            }
                        }
                    }
                }
            }

            HttpResponse::Ok().json(combined_members)
        },
        Ok(None) => HttpResponse::Unauthorized().body("You are not a member of this team"),
        Err(err) => HttpResponse::InternalServerError()
            .body(format!("Error checking membership: {}", err)),
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
                    update_doc.get_document_mut("$set").unwrap().insert("owner_id", new_owner);
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
        },
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
        Ok(Some(_)) => {}
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
        },
        Err(e) => HttpResponse::InternalServerError().body(format!("Error removing member: {}", e)),
    }
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

    if let Ok(Some(_)) = user_teams_collection.find_one(membership_filter.clone()).await {
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
        },
        Ok(None) => HttpResponse::Unauthorized().body("Only team admins can delete invitations"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error verifying admin status: {}", e)),
    }
}
