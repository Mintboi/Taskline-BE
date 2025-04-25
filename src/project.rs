// src/project.rs

use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use chrono::Utc;
use futures_util::StreamExt;
use mongodb::bson::{doc, to_document};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use log::{debug, error, info};

use crate::app_state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub team_id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub created_by: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectMembership {
    pub project_id: String,
    pub user_id: String,
    pub role: String,
    pub joined_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssignUserRequest {
    pub user_id: String,
    pub role: String,
}

/// POST /teams/{team_id}/projects
/// Creates a new project within a team.
pub async fn create_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
    project_info: web::Json<CreateProjectRequest>,
) -> impl Responder {
    debug!(
        "Received create_project request for team_id: {} with payload: {:?}",
        team_id, project_info
    );
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        error!("Unauthorized in create_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // 1) Verify team membership
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let team_filter = doc! { "team_id": &*team_id, "user_id": &current_user };
    match user_teams.find_one(team_filter).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            error!("User {} not in team {}", current_user, team_id);
            return HttpResponse::Unauthorized().body("Not a member of the team");
        }
        Err(e) => {
            error!("Error checking membership: {}", e);
            return HttpResponse::InternalServerError().body("Error checking membership");
        }
    }

    // 2) Insert project
    let new_project = Project {
        project_id: Uuid::new_v4().to_string(),
        team_id: team_id.into_inner(),
        name: project_info.name.clone(),
        description: project_info.description.clone(),
        created_at: Utc::now(),
        created_by: current_user.clone(),
    };
    let projects_coll = data.mongodb.db.collection::<Project>("projects");
    if let Err(e) = projects_coll.insert_one(&new_project).await {
        error!("Error creating project: {}", e);
        return HttpResponse::InternalServerError().body("Error creating project");
    }
    info!("Project created {:?}", new_project.project_id);

    // 3) Seed project_memberships
    let proj_members = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let membership = ProjectMembership {
        project_id: new_project.project_id.clone(),
        user_id: current_user.clone(),
        role: "owner".to_string(),
        joined_at: Utc::now(),
    };
    let membership_doc = match to_document(&membership) {
        Ok(doc) => doc,
        Err(e) => {
            error!("Error serializing membership: {}", e);
            return HttpResponse::InternalServerError().body("Error adding membership");
        }
    };
    if let Err(e) = proj_members.insert_one(membership_doc).await {
        error!("Error inserting membership: {}", e);
        return HttpResponse::InternalServerError().body("Error adding membership");
    }

    HttpResponse::Ok().json(new_project)
}

/// GET /teams/{team_id}/projects
pub async fn list_projects(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let team_id = team_id.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        error!("Unauthorized in list_projects");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify team membership
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    if user_teams
        .find_one(doc! { "team_id": &team_id, "user_id": &current_user })
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Not a member of the team");
    }

    // Fetch and return
    let projects_coll = data.mongodb.db.collection::<Project>("projects");
    let mut cursor = match projects_coll.find(doc! { "team_id": &team_id }).await {
        Ok(c) => c,
        Err(e) => {
            error!("Error fetching projects: {}", e);
            return HttpResponse::InternalServerError().body("Error fetching projects");
        }
    };
    let mut projects = Vec::new();
    while let Some(res) = cursor.next().await {
        match res {
            Ok(p) => projects.push(p),
            Err(e) => {
                error!("Cursor error: {}", e);
                return HttpResponse::InternalServerError().body("Error reading projects");
            }
        }
    }
    HttpResponse::Ok().json(projects)
}

/// GET /teams/{team_id}/projects/{project_id}
pub async fn get_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>,
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        error!("Unauthorized in get_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify team membership
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    if user_teams
        .find_one(doc! { "team_id": &team_id, "user_id": &current_user })
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Not a member of the team");
    }

    // Fetch project
    let projects_coll = data.mongodb.db.collection::<Project>("projects");
    match projects_coll
        .find_one(doc! { "team_id": &team_id, "project_id": &project_id })
        .await
    {
        Ok(Some(proj)) => HttpResponse::Ok().json(proj),
        Ok(None) => HttpResponse::NotFound().body("Project not found"),
        Err(e) => {
            error!("Error fetching project: {}", e);
            HttpResponse::InternalServerError().body("Error fetching project")
        }
    }
}

/// PUT /teams/{team_id}/projects/{project_id}
pub async fn update_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>,
    update_info: web::Json<UpdateProjectRequest>,
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        error!("Unauthorized in update_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify project ownership
    let memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    if memberships
        .find_one(
            doc! { "project_id": &project_id, "user_id": &current_user, "role": "owner" },
            
        )
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Only project owner can update");
    }

    // Build update doc
    let mut set_doc = doc! {};
    if let Some(name) = &update_info.name {
        set_doc.insert("name", name.clone());
    }
    if let Some(desc) = &update_info.description {
        set_doc.insert("description", desc.clone());
    }
    if set_doc.is_empty() {
        return HttpResponse::BadRequest().body("No fields to update");
    }

    let projects_coll = data.mongodb.db.collection::<Project>("projects");
    match projects_coll
        .update_one(
            doc! { "team_id": &team_id, "project_id": &project_id },
            doc! { "$set": set_doc },
            
        )
        .await
    {
        Ok(res) if res.matched_count == 1 => HttpResponse::Ok().body("Project updated"),
        Ok(_) => HttpResponse::NotFound().body("Project not found"),
        Err(e) => {
            error!("Error updating project: {}", e);
            HttpResponse::InternalServerError().body("Error updating project")
        }
    }
}

/// DELETE /teams/{team_id}/projects/{project_id}
pub async fn delete_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>,
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        error!("Unauthorized in delete_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify project ownership
    let memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    if memberships
        .find_one(
            doc! {
                "project_id": &project_id,
                "user_id": &current_user,
                "role": "owner"
            },
            
        )
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Only project owner can delete");
    }

    // Delete
    let projects_coll = data.mongodb.db.collection::<Project>("projects");
    match projects_coll
        .delete_one(doc! { "team_id": &team_id, "project_id": &project_id })
        .await
    {
        Ok(res) if res.deleted_count == 1 => HttpResponse::Ok().body("Project deleted"),
        Ok(_) => HttpResponse::NotFound().body("Project not found"),
        Err(e) => {
            error!("Error deleting project: {}", e);
            HttpResponse::InternalServerError().body("Error deleting project")
        }
    }
}

/// POST /teams/{team_id}/projects/{project_id}/members
pub async fn add_user_to_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
    payload: web::Json<AssignUserRequest>,
) -> impl Responder {
    let (team_id, project_id) = path.into_inner();
    let current_user = if let Some(uid) = req.extensions().get::<String>() {
        uid.clone()
    } else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // 1) Only project owner may add
    let proj_members = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    if proj_members
        .find_one(
            doc! { "project_id": &project_id, "user_id": &current_user, "role": "owner" },
            
        )
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::Unauthorized().body("Only project owner can add members");
    }

    // 2) Target must be in team
    let team_coll = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    if team_coll
        .find_one(doc! { "team_id": &team_id, "user_id": &payload.user_id })
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return HttpResponse::BadRequest().body("User not a member of the team");
    }

    // 3) Prevent duplicates
    if proj_members
        .find_one(
            doc! { "project_id": &project_id, "user_id": &payload.user_id },
            
        )
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return HttpResponse::BadRequest().body("User already in project");
    }

    // 4) Insert membership
    let new_mem = ProjectMembership {
        project_id: project_id.clone(),
        user_id: payload.user_id.clone(),
        role: payload.role.clone(),
        joined_at: Utc::now(),
    };
    let doc = match to_document(&new_mem) {
        Ok(d) => d,
        Err(e) => {
            error!("Serialize error: {}", e);
            return HttpResponse::InternalServerError().body("Error adding user");
        }
    };
    if let Err(e) = proj_members.insert_one(doc).await {
        error!("DB error: {}", e);
        return HttpResponse::InternalServerError().body("Error adding user");
    }

    info!("Added {} to project {}", payload.user_id, project_id);
    HttpResponse::Ok().body("User added to project")
}
