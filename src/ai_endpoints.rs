use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use crate::app_state::AppState;

#[derive(Deserialize)]
pub struct TaskInput {
    tasks: Vec<String>,
    priorities: Vec<i32>,
}

#[derive(Serialize)]
pub struct PrioritizedTask {
    task: String,
    priority: i32,
}


pub async fn prioritize_tasks(data: web::Data<AppState>, req: web::Json<TaskInput>) -> impl Responder {
    let prioritized: Vec<PrioritizedTask> = req
        .tasks
        .iter()
        .zip(&req.priorities)
        .map(|(task, &priority)| PrioritizedTask {
            task: task.clone(),
            priority: priority + 10, // Mock AI adjustment
        })
        .collect();

    HttpResponse::Ok().json(prioritized)
}

pub async fn get_team_morale(_data: web::Data<AppState>, team_id: web::Path<String>) -> impl Responder {
    // Just returning a dummy morale score for now
    HttpResponse::Ok().json(serde_json::json!({ "team_id": *team_id, "morale_score": 0.8 }))
}

