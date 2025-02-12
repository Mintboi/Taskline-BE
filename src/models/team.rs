use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;
use chrono::Utc;
use futures_util::StreamExt;
use mongodb::bson::{doc, Bson, DateTime as BsonDateTime};
use mongodb::options::FindOptions;

use crate::app_state::AppState;
use crate::models::task::{Task, CreateTaskRequest, UpdateTaskRequest};

pub async fn create_task(data: web::Data<AppState>, req: web::Json<CreateTaskRequest>) -> impl Responder {
    let tasks_coll = data.mongodb.db.collection::<Task>("tasks");
    let new_task = Task {
        task_id: Uuid::new_v4(),
        team_id: req.team_id.clone(),
        title: req.title.clone(),
        description: req.description.clone(),
        priority: 0,
        assignee_id: None,
        status: "todo".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    if let Err(e) = tasks_coll.insert_one(&new_task).await {
        return HttpResponse::InternalServerError().body(format!("Error creating task: {:?}", e));
    }

    HttpResponse::Ok().json(&new_task)
}

pub async fn get_tasks_by_team(data: web::Data<AppState>, team_id: web::Path<String>) -> impl Responder {
    let tasks_coll = data.mongodb.db.collection::<Task>("tasks");
    let filter = doc! { "team_id": &*team_id };
    let mut cursor = match tasks_coll.find(filter).await {
        Ok(cursor) => cursor,
        Err(e) => return HttpResponse::InternalServerError().body(format!("Error fetching tasks: {:?}", e)),
    };

    let mut tasks = Vec::new();
    while let Some(task_res) = cursor.next().await {
        if let Ok(task) = task_res {
            tasks.push(task);
        }
    }

    HttpResponse::Ok().json(tasks)
}

pub async fn update_task(
    data: web::Data<AppState>,
    path: web::Path<Uuid>,
    req: web::Json<UpdateTaskRequest>
) -> impl Responder {
    let tasks_coll = data.mongodb.db.collection::<Task>("tasks");
    let task_id_bson = Bson::String(path.to_string());

    let mut update_doc = doc!{};
    if let Some(title) = &req.title {
        update_doc.insert("title", title);
    }
    if let Some(desc) = &req.description {
        update_doc.insert("description", desc);
    }
    if let Some(priority) = &req.priority {
        update_doc.insert("priority", priority);
    }
    if let Some(assignee) = &req.assignee_id {
        update_doc.insert("assignee_id", assignee);
    }
    if let Some(status) = &req.status {
        update_doc.insert("status", status);
    }

    if update_doc.is_empty() {
        return HttpResponse::BadRequest().body("No fields to update");
    }

    update_doc.insert("updated_at", BsonDateTime::from_millis(Utc::now().timestamp_millis()));

    match tasks_coll.update_one(doc! {"_id": task_id_bson}, doc!{"$set": update_doc}).await {
        Ok(res) => {
            if res.matched_count == 0 {
                HttpResponse::NotFound().body("Task not found")
            } else {
                HttpResponse::Ok().body("Task updated")
            }
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("Error updating task: {:?}", e))
    }
}

pub async fn delete_task(data: web::Data<AppState>, task_id: web::Path<Uuid>) -> impl Responder {
    let tasks_collection = data.mongodb.db.collection::<Uuid>("tasks");
    match tasks_collection.delete_one(doc! { "_id": task_id.to_string() }).await {
        Ok(_) => HttpResponse::Ok().body("Task deleted"),
        Err(_) => HttpResponse::InternalServerError().body("Failed to delete task"),
    }
}
