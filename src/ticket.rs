// src/ticket.rs

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use futures_util::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{Utc, DateTime};
use log::{error, info};

use crate::app_state::AppState;

/// The Ticket model, expanded with optional fields like sprint, reporter, assignee, etc.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ticket {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub ticket_id: String,

    pub board_id: String,
    pub project_id: String,

    pub title: String,
    pub description: Option<String>,

    /// e.g. "To Do", "In Progress", "Blocked", "Done", etc.
    pub status: String,

    /// e.g. "High", "Medium", "Low", or "Normal"
    pub priority: Option<String>,

    /// The user who created the ticket. (Default empty string for legacy documents)
    #[serde(default)]
    pub reporter: String,

    /// The user who’s assigned to the ticket (optional)
    pub assignee: Option<String>,

    /// The date by which the ticket should be completed (optional)
    pub due_date: Option<DateTime<Utc>>,

    /// e.g. "Task", "Story", "Bug", etc.
    pub ticket_type: Option<String>,

    /// A numeric sprint indicator, if you are using sprints
    pub sprint: Option<i32>,

    /// Arbitrary labels
    pub labels: Option<Vec<String>>,

    /// Attachments or file URLs
    pub attachments: Option<Vec<String>>,

    /// Simple comments
    pub comments: Option<Vec<TicketComment>>,

    pub created_at: DateTime<Utc>,
}

/// A small struct for comments
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketComment {
    pub author_id: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Request payload for creating a ticket
#[derive(Debug, Deserialize)]
pub struct CreateTicketRequest {
    pub board_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub ticket_type: Option<String>,
    pub sprint: Option<i32>,
    pub labels: Option<Vec<String>>,
    pub attachments: Option<Vec<String>>,
}

/// Request payload for updating a ticket
#[derive(Debug, Deserialize)]
pub struct UpdateTicketRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub ticket_type: Option<String>,
    pub sprint: Option<i32>,
    pub labels: Option<Vec<String>>,
    pub attachments: Option<Vec<String>>,
}

/// CREATE a new ticket
pub async fn create_ticket(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String)>, // (team_id, project_id)
    payload: web::Json<CreateTicketRequest>,
) -> impl Responder {
    let (team_id, project_id) = path.into_inner();
    let current_user = match req.extensions().get::<String>() {
        Some(uid) => uid.clone(),
        None => return HttpResponse::Unauthorized().body("Unauthorized"),
    };

    // 1) Check if user is a member of the team.
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let filter_member = doc! { "team_id": &team_id, "user_id": &current_user };
    if user_teams.find_one(filter_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }

    // 2) Check if user is a member of the project.
    let project_memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let filter_project_member = doc! { "project_id": &project_id, "user_id": &current_user };
    if project_memberships.find_one(filter_project_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this project");
    }

    // 3) If there's an assignee, confirm that user is also a team member
    if let Some(assignee_id) = &payload.assignee {
        let filter_assignee = doc! { "team_id": &team_id, "user_id": assignee_id };
        if user_teams.find_one(filter_assignee).await.ok().flatten().is_none() {
            return HttpResponse::BadRequest().body("Assignee must be a member of the same team");
        }
    }

    // 4) Create the new ticket.
    let new_ticket = Ticket {
        id: None,
        ticket_id: Uuid::new_v4().to_string(),
        board_id: payload.board_id.clone(),
        project_id: project_id.clone(),
        title: payload.title.clone(),
        description: payload.description.clone(),
        status: payload.status.clone().unwrap_or_else(|| "To Do".to_string()),
        priority: payload.priority.clone(),
        reporter: current_user.clone(), // set automatically
        assignee: payload.assignee.clone(),
        due_date: payload.due_date.clone(),
        ticket_type: payload.ticket_type.clone(),
        sprint: payload.sprint,
        labels: payload.labels.clone(),
        attachments: payload.attachments.clone(),
        comments: Some(vec![]),
        created_at: Utc::now(),
    };

    let tickets_coll = data.mongodb.db.collection::<Ticket>("tickets");
    match tickets_coll.insert_one(&new_ticket).await {
        Ok(_) => {
            info!("Ticket created: {:?}", new_ticket.ticket_id);
            HttpResponse::Ok().json(&new_ticket)
        },
        Err(e) => {
            error!("Error inserting ticket: {}", e);
            HttpResponse::InternalServerError().body("Error inserting ticket")
        }
    }
}

/// GET a single ticket
pub async fn get_ticket(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>, // (team_id, project_id, ticket_id)
) -> impl Responder {
    let (team_id, project_id, ticket_id) = path.into_inner();
    let current_user = match req.extensions().get::<String>() {
        Some(uid) => uid.clone(),
        None => return HttpResponse::Unauthorized().body("Unauthorized"),
    };

    // Check membership in team and project
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let filter_member = doc! { "team_id": &team_id, "user_id": &current_user };
    if user_teams.find_one(filter_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }
    let project_memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let filter_project_member = doc! { "project_id": &project_id, "user_id": &current_user };
    if project_memberships.find_one(filter_project_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this project");
    }

    let tickets_coll = data.mongodb.db.collection::<Ticket>("tickets");
    let filter = doc! { "ticket_id": &ticket_id, "project_id": &project_id };
    match tickets_coll.find_one(filter).await {
        Ok(Some(ticket)) => HttpResponse::Ok().json(ticket),
        Ok(None) => HttpResponse::NotFound().body("Ticket not found"),
        Err(e) => {
            error!("Error fetching ticket: {}", e);
            HttpResponse::InternalServerError().body("Error fetching ticket")
        }
    }
}

/// UPDATE an existing ticket
pub async fn update_ticket(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>, // (team_id, project_id, ticket_id)
    payload: web::Json<UpdateTicketRequest>,
) -> impl Responder {
    let (team_id, project_id, ticket_id) = path.into_inner();
    let current_user = match req.extensions().get::<String>() {
        Some(uid) => uid.clone(),
        None => return HttpResponse::Unauthorized().body("Unauthorized"),
    };

    // Check membership
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let filter_member = doc! { "team_id": &team_id, "user_id": &current_user };
    if user_teams.find_one(filter_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }
    let project_memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let filter_project_member = doc! { "project_id": &project_id, "user_id": &current_user };
    if project_memberships.find_one(filter_project_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this project");
    }

    // If there's an assignee, check membership as well.
    if let Some(assignee_id) = &payload.assignee {
        let filter_assignee = doc! { "team_id": &team_id, "user_id": assignee_id };
        if user_teams.find_one(filter_assignee).await.ok().flatten().is_none() {
            return HttpResponse::BadRequest().body("Assignee must be a member of the same team");
        }
    }

    let tickets_coll = data.mongodb.db.collection::<Ticket>("tickets");
    let filter = doc! { "ticket_id": &ticket_id, "project_id": &project_id };

    let mut update_doc = doc! {};
    if let Some(title) = &payload.title { update_doc.insert("title", title); }
    if let Some(description) = &payload.description { update_doc.insert("description", description); }
    if let Some(status) = &payload.status { update_doc.insert("status", status); }
    if let Some(priority) = &payload.priority { update_doc.insert("priority", priority); }
    if let Some(assignee) = &payload.assignee { update_doc.insert("assignee", assignee); }
    if let Some(due_date) = &payload.due_date {
        // Convert due_date to milliseconds and then to BSON DateTime
        update_doc.insert("due_date", BsonDateTime::from_millis(due_date.timestamp_millis()));
    }
    if let Some(ticket_type) = &payload.ticket_type { update_doc.insert("ticket_type", ticket_type); }
    if let Some(sprint) = &payload.sprint { update_doc.insert("sprint", sprint); }
    if let Some(labels) = &payload.labels { update_doc.insert("labels", labels); }
    if let Some(attachments) = &payload.attachments { update_doc.insert("attachments", attachments); }

    if update_doc.is_empty() {
        return HttpResponse::BadRequest().body("No fields to update");
    }

    let update_op = doc! { "$set": update_doc };
    match tickets_coll.update_one(filter, update_op).await {
        Ok(res) => {
            if res.matched_count == 0 {
                HttpResponse::NotFound().body("Ticket not found")
            } else {
                HttpResponse::Ok().body("Ticket updated successfully")
            }
        },
        Err(e) => {
            error!("Error updating ticket: {}", e);
            HttpResponse::InternalServerError().body("Error updating ticket")
        }
    }
}

/// DELETE a ticket
pub async fn delete_ticket(
    req: HttpRequest,
    data: web::Data<AppState>,
    path: web::Path<(String, String, String)>, // (team_id, project_id, ticket_id)
) -> impl Responder {
    let (team_id, project_id, ticket_id) = path.into_inner();
    let current_user = match req.extensions().get::<String>() {
        Some(uid) => uid.clone(),
        None => return HttpResponse::Unauthorized().body("Unauthorized"),
    };

    // Check membership
    let user_teams = data.mongodb.db.collection::<mongodb::bson::Document>("user_teams");
    let filter_member = doc! { "team_id": &team_id, "user_id": &current_user };
    if user_teams.find_one(filter_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this team");
    }
    let project_memberships = data.mongodb.db.collection::<mongodb::bson::Document>("project_memberships");
    let filter_project_member = doc! { "project_id": &project_id, "user_id": &current_user };
    if project_memberships.find_one(filter_project_member).await.ok().flatten().is_none() {
        return HttpResponse::Unauthorized().body("Not a member of this project");
    }

    let tickets_coll = data.mongodb.db.collection::<Ticket>("tickets");
    let filter = doc! { "ticket_id": &ticket_id, "project_id": &project_id };
    match tickets_coll.delete_one(filter).await {
        Ok(res) => {
            if res.deleted_count == 0 {
                HttpResponse::NotFound().body("Ticket not found or already deleted")
            } else {
                HttpResponse::Ok().body("Ticket deleted successfully")
            }
        },
        Err(e) => {
            error!("Error deleting ticket: {}", e);
            HttpResponse::InternalServerError().body("Error deleting ticket")
        }
    }
}

/// LIST tickets for a given board
#[derive(Debug, Deserialize)]
pub struct TicketQuery {
    pub board_id: String,
}

pub async fn list_tickets(
    _req: HttpRequest,
    data: web::Data<AppState>,
    query: web::Query<TicketQuery>,
) -> impl Responder {
    let tickets_coll = data.mongodb.db.collection::<Ticket>("tickets");
    let filter = doc! { "board_id": &query.board_id };
    let mut cursor = match tickets_coll.find(filter).await {
        Ok(cur) => cur,
        Err(e) => {
            error!("Error fetching tickets: {}", e);
            return HttpResponse::InternalServerError().body("Error fetching tickets");
        }
    };

    let mut tickets = vec![];
    while let Some(ticket_res) = cursor.next().await {
        match ticket_res {
            Ok(ticket) => tickets.push(ticket),
            Err(e) => {
                error!("Error reading tickets: {}", e);
                return HttpResponse::InternalServerError().body("Error reading tickets");
            }
        }
    }
    HttpResponse::Ok().json(tickets)
}
