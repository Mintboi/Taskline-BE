use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use mongodb::bson::{doc, Bson, Uuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid as UuidV4;
use crate::AppState;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Team {
    pub team_id: String,
    pub name: String,
    pub owner_id: String,
    pub members: Vec<TeamMember>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TeamMember {
    pub user_id: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct TeamInfo {
    pub name: String,
    pub owner_id: String,
}

#[derive(Deserialize)]
pub struct InvitationInfo {
    pub team_id: Uuid,
    pub email: String,
}

#[derive(Deserialize)]
pub struct UpdateTeamInfo {
    pub name: Option<String>,
}

// Create a new team
pub async fn create_team(
    data: web::Data<AppState>,
    team_info: web::Json<TeamInfo>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let team_id = UuidV4::new_v4().to_string();

    let new_team = Team {
        team_id: team_id.clone(),
        name: team_info.name.clone(),
        owner_id: team_info.owner_id.clone(),
        members: vec![TeamMember {
            user_id: team_info.owner_id.clone(),
            email: "owner_email_placeholder@domain.com".to_string(), // Replace later with actual user lookup
        }],
        created_at: Utc::now(),
    };

    match teams_collection.insert_one(&new_team, None).await {
        Ok(_) => HttpResponse::Ok().json(new_team),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error creating team: {:?}", e)),
    }
}

// Invite a user to a team
pub async fn invite_user(
    data: web::Data<AppState>,
    invite_info: web::Json<InvitationInfo>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let users_collection = data.mongodb.db.collection::<crate::User>("users");

    // Find user by email
    let user = match users_collection.find_one(doc! { "email": &invite_info.email }, None).await {
        Ok(Some(user)) => user,
        Ok(None) => return HttpResponse::BadRequest().body("User not found"),
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching user: {:?}", e)),
    };

    let team_id = invite_info.team_id.to_string();
    match teams_collection.find_one(doc! { "team_id": &team_id }, None).await {
        Ok(Some(mut team)) => {
            // Check if the user is already in the team
            if team.members.iter().any(|m| m.user_id == user.user_id) {
                return HttpResponse::BadRequest().body("User already a team member");
            }

            // Add user to the team
            let new_member = TeamMember {
                user_id: user.user_id.clone(),
                email: invite_info.email.clone(),
            };
            team.members.push(new_member);

            let update = doc! { "$set": { "members": &team.members } };
            match teams_collection.update_one(doc! { "team_id": &team_id }, update, None).await {
                Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "status": "User invited" })),
                Err(e) => HttpResponse::InternalServerError().body(format!("Error updating team: {:?}", e)),
            }
        }
        Ok(None) => HttpResponse::NotFound().body("Team not found"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error fetching team: {:?}", e)),
    }
}

// Fetch teams for a user
pub async fn get_user_teams(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let filter = doc! { "members.user_id": &*user_id }; // Query by user_id in members

    let mut cursor = match teams_collection.find(filter, None).await {
        Ok(cursor) => cursor,
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching teams: {:?}", e)),
    };

    let mut teams = Vec::new();
    while let Some(team) = cursor.next().await {
        if let Ok(t) = team {
            teams.push(t);
        }
    }

    HttpResponse::Ok().json(teams)
}

// Fetch team members
pub async fn get_team_members(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");

    match teams_collection.find_one(doc! { "team_id": &*team_id }, None).await {
        Ok(Some(team)) => HttpResponse::Ok().json(team.members),
        Ok(None) => HttpResponse::NotFound().body("Team not found"),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error fetching team members: {:?}", e)),
    }
}

// Update team information
pub async fn update_team(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
    update_info: web::Json<UpdateTeamInfo>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");
    let mut update_doc = doc! {};

    if let Some(name) = &update_info.name {
        update_doc.insert("name", name);
    }

    if update_doc.is_empty() {
        return HttpResponse::BadRequest().body("No fields to update");
    }

    let update = doc! { "$set": update_doc };
    match teams_collection.update_one(doc! { "team_id": &*team_id }, update, None).await {
        Ok(res) => {
            if res.matched_count == 0 {
                HttpResponse::NotFound().body("Team not found")
            } else {
                HttpResponse::Ok().body("Team updated")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating team: {:?}", e)),
    }
}

// Delete a team
pub async fn delete_team(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let teams_collection = data.mongodb.db.collection::<Team>("teams");

    match teams_collection.delete_one(doc! { "team_id": &*team_id }, None).await {
        Ok(res) => {
            if res.deleted_count == 0 {
                HttpResponse::NotFound().body("Team not found")
            } else {
                HttpResponse::Ok().body("Team deleted")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error deleting team: {:?}", e)),
    }
}
