// project.rs

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

// Payload for creating a project. Note: team_id is taken from the URL.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

// Payload for updating a project.
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

// Payload for removing a project member.
#[derive(Debug, Deserialize)]
pub struct RemoveProjectMemberRequest {
    pub project_id: String,
    pub user_id: String,
}

// Payload for assigning a user to a project.
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
    team_id: web::Path<String>,             // Extract team_id from the URL.
    project_info: web::Json<CreateProjectRequest>,
) -> impl Responder {
    debug!(
        "Received create_project request for team_id: {} with payload: {:?}",
        team_id, project_info
    );
    let current_user = if let Some(user_id) = req.extensions().get::<String>() {
        user_id.clone()
    } else {
        error!("User not authenticated in create_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    info!("User {} attempting to create project", current_user);

    // Check that the user is a member of the team.
    let user_teams_collection = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let team_filter = doc! { "team_id": &*team_id, "user_id": &current_user };
    debug!(
        "Checking team membership for team {} and user {}",
        team_id, current_user
    );
    match user_teams_collection.find_one(team_filter).await {
        Ok(Some(doc)) => {
            debug!("Team membership found: {:?}", doc);
        },
        Ok(None) => {
            error!("User {} is not a member of team {}", current_user, team_id);
            return HttpResponse::Unauthorized().body("User is not a member of the specified team");
        },
        Err(e) => {
            error!("Error checking team membership: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error checking team membership: {}", e));
        },
    }

    // Create new project using team_id from the URL.
    let new_project = Project {
        project_id: Uuid::new_v4().to_string(),
        team_id: team_id.into_inner(),
        name: project_info.name.clone(),
        description: project_info.description.clone(),
        created_at: Utc::now(),
        created_by: current_user.clone(),
    };
    debug!("New project being created: {:?}", new_project);

    let projects_collection = data.mongodb.db.collection::<Project>("projects");
    match projects_collection.insert_one(&new_project).await {
        Ok(insert_result) => {
            debug!("Project inserted successfully: {:?}", insert_result);
            // Insert project membership record.
            let project_memberships_collection = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
            let membership = ProjectMembership {
                project_id: new_project.project_id.clone(),
                user_id: current_user,
                role: "owner".to_string(),
                joined_at: Utc::now(),
            };
            let membership_doc = match to_document(&membership) {
                Ok(doc) => {
                    debug!("Converted membership record: {:?}", doc);
                    doc
                },
                Err(e) => {
                    error!("Error converting membership: {}", e);
                    return HttpResponse::InternalServerError().body(format!("Error converting membership: {}", e));
                },
            };
            if let Err(e) = project_memberships_collection.insert_one(membership_doc).await {
                error!("Error inserting project membership: {}", e);
                return HttpResponse::InternalServerError().body(format!("Error adding project membership: {}", e));
            }
            info!("Project created successfully with ID: {}", new_project.project_id);
            HttpResponse::Ok().json(new_project)
        },
        Err(e) => {
            error!("Error creating project: {}", e);
            HttpResponse::InternalServerError().body(format!("Error creating project: {}", e))
        },
    }
}

/// GET /teams/{team_id}/projects
/// Lists all projects in a team.
pub async fn list_projects(
    req: HttpRequest,
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let team_id_str = team_id.into_inner();
    debug!("Listing projects for team: {}", team_id_str);
    let current_user = if let Some(user_id) = req.extensions().get::<String>() {
        user_id.clone()
    } else {
        error!("Unauthorized access in list_projects");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };
    // Verify membership in the team.
    let user_teams_collection = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let team_filter = doc! { "team_id": &team_id_str, "user_id": &current_user };
    match user_teams_collection.find_one(team_filter).await {
        Ok(Some(_)) => {
            debug!("User {} is a member of team {}", current_user, team_id_str);
        },
        Ok(None) => {
            error!("User {} is not a member of team {}", current_user, team_id_str);
            return HttpResponse::Unauthorized().body("User is not a member of this team");
        },
        Err(e) => {
            error!("Error checking membership: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error checking membership: {}", e));
        },
    }
    let projects_collection = data.mongodb.db.collection::<Project>("projects");
    let filter = doc! { "team_id": &team_id_str };
    let mut cursor = match projects_collection.find(filter).await {
        Ok(cursor) => {
            debug!("Successfully obtained cursor for projects");
            cursor
        },
        Err(e) => {
            error!("Error fetching projects: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error fetching projects: {}", e));
        },
    };
    let mut projects = Vec::new();
    while let Some(project_result) = cursor.next().await {
        match project_result {
            Ok(project) => {
                debug!("Fetched project: {:?}", project);
                projects.push(project);
            },
            Err(e) => {
                error!("Error iterating projects: {}", e);
                return HttpResponse::InternalServerError().body(format!("Error iterating projects: {}", e));
            },
        }
    }
    info!("Total projects found: {}", projects.len());
    HttpResponse::Ok().json(projects)
}

/// GET /teams/{team_id}/projects/{project_id}
/// Retrieves details for a single project.
pub async fn get_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>, // (team_id, project_id)
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    debug!("Getting project {} for team {}", project_id, team_id);
    let current_user = if let Some(user_id) = req.extensions().get::<String>() {
        user_id.clone()
    } else {
        error!("Unauthorized access in get_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Verify that the user is a member of the team.
    let user_teams_collection = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let team_filter = doc! { "team_id": &team_id, "user_id": &current_user };
    match user_teams_collection.find_one(team_filter).await {
        Ok(Some(_)) => {
            debug!("User {} is a member of team {}", current_user, team_id);
        },
        Ok(None) => {
            error!("User {} is not a member of team {}", current_user, team_id);
            return HttpResponse::Unauthorized().body("User is not a member of this team");
        },
        Err(e) => {
            error!("Error checking team membership: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error checking team membership: {}", e));
        },
    }

    let projects_collection = data.mongodb.db.collection::<Project>("projects");
    let filter = doc! { "project_id": &project_id, "team_id": &team_id };
    match projects_collection.find_one(filter).await {
        Ok(Some(project)) => {
            debug!("Project found: {:?}", project);
            HttpResponse::Ok().json(project)
        },
        Ok(None) => {
            error!("Project not found for team {} with project_id {}", team_id, project_id);
            HttpResponse::NotFound().body("Project not found")
        },
        Err(e) => {
            error!("Error fetching project: {}", e);
            HttpResponse::InternalServerError().body(format!("Error fetching project: {}", e))
        },
    }
}

/// PUT /teams/{team_id}/projects/{project_id}
/// Updates project details.
pub async fn update_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>, // (team_id, project_id)
    update_info: web::Json<UpdateProjectRequest>,
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    debug!("Update project request for project_id: {} in team {} with payload: {:?}", project_id, team_id, update_info);
    let current_user = if let Some(id) = req.extensions().get::<String>() { id.clone() } else {
        error!("Unauthorized access in update_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Check membership and ensure the current user is the project owner.
    let project_memberships_collection = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let membership_filter = doc! { "project_id": &project_id, "user_id": &current_user };
    let membership_doc = match project_memberships_collection.find_one(membership_filter).await {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            error!("User {} is not a member of project {}", current_user, project_id);
            return HttpResponse::Unauthorized().body("User is not a member of this project");
        },
        Err(e) => {
            error!("Error checking project membership: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error checking project membership: {}", e));
        },
    };
    if let Some(role) = membership_doc.get_str("role").ok() {
        if role != "owner" {
            error!("User {} is not the owner of project {}", current_user, project_id);
            return HttpResponse::Unauthorized().body("Only project owner can update the project");
        }
    } else {
        error!("Role not found in membership for project {}", project_id);
        return HttpResponse::InternalServerError().body("Role not found in membership");
    }

    // Build update document based on provided fields.
    let mut update_doc = mongodb::bson::Document::new();
    if let Some(name) = &update_info.name {
        update_doc.insert("name", name);
    }
    if let Some(description) = &update_info.description {
        update_doc.insert("description", description);
    }
    if update_doc.is_empty() {
        error!("No fields provided to update for project {}", project_id);
        return HttpResponse::BadRequest().body("No fields to update");
    }
    let update = doc! { "$set": update_doc };
    let projects_collection = data.mongodb.db.collection::<Project>("projects");
    let project_filter = doc! { "project_id": &project_id, "team_id": &team_id };
    match projects_collection.update_one(project_filter, update).await {
        Ok(result) => {
            info!("Project {} updated successfully: {:?}", project_id, result);
            HttpResponse::Ok().body("Project updated successfully")
        },
        Err(e) => {
            error!("Error updating project {}: {}", project_id, e);
            HttpResponse::InternalServerError().body(format!("Error updating project: {}", e))
        },
    }
}

/// DELETE /teams/{team_id}/projects/{project_id}
/// Deletes a project.
pub async fn delete_project(
    req: HttpRequest,
    data: web::Data<AppState>,
    params: web::Path<(String, String)>, // (team_id, project_id)
) -> impl Responder {
    let (team_id, project_id) = params.into_inner();
    debug!("Delete project request for project_id: {} in team {}", project_id, team_id);
    let current_user = if let Some(id) = req.extensions().get::<String>() { id.clone() } else {
        error!("Unauthorized access in delete_project");
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    // Check if current user is project owner.
    let project_memberships_collection = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let membership_filter = doc! { "project_id": &project_id, "user_id": &current_user };
    let membership_doc = match project_memberships_collection.find_one(membership_filter).await {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            error!("User {} is not a member of project {}", current_user, project_id);
            return HttpResponse::Unauthorized().body("User is not a member of this project");
        },
        Err(e) => {
            error!("Error checking project membership: {}", e);
            return HttpResponse::InternalServerError().body(format!("Error checking project membership: {}", e));
        },
    };
    if let Some(role) = membership_doc.get_str("role").ok() {
        if role != "owner" {
            error!("User {} is not the owner of project {}", current_user, project_id);
            return HttpResponse::Unauthorized().body("Only project owner can delete the project");
        }
    } else {
        error!("Role not found in membership for project {}", project_id);
        return HttpResponse::InternalServerError().body("Role not found in membership");
    }
    let projects_collection = data.mongodb.db.collection::<Project>("projects");
    let project_filter = doc! { "project_id": &project_id, "team_id": &team_id };
    match projects_collection.delete_one(project_filter).await {
        Ok(result) => {
            info!("Project {} deleted successfully: {:?}", project_id, result);
            let _ = project_memberships_collection.delete_many(doc! { "project_id": &project_id }).await;
            HttpResponse::Ok().body("Project deleted successfully")
        },
        Err(e) => {
            error!("Error deleting project {}: {}", project_id, e);
            HttpResponse::InternalServerError().body(format!("Error deleting project: {}", e))
        },
    }
}
