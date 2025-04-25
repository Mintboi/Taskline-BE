use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use crate::app_state::AppState;

#[derive(Deserialize, Serialize)]
pub struct TaskInput {
    pub tasks: Vec<String>,
    pub priorities: Vec<i32>,
}

#[derive(Serialize, Deserialize)]
pub struct PrioritizedTask {
    pub task: String,
    pub priority: i32,
}

pub async fn prioritize_tasks(
    data: web::Data<AppState>,
    req: web::Json<TaskInput>,
) -> impl Responder {
    // decide which endpoint to call
    let endpoint = if data.config.ai_use_local {
        &data.config.ai_local_endpoint
    } else {
        &data.config.ai_aws_endpoint
    };
    let url = format!("{}/prioritize", endpoint.trim_end_matches('/'));

    match data.http_client.post(&url)
        .json(&*req)
        .send()
        .await
    {
        Ok(mut resp) if resp.status().is_success() => {
            match resp.json::<Vec<PrioritizedTask>>().await {
                Ok(ts) => HttpResponse::Ok().json(ts),
                Err(e) => HttpResponse::InternalServerError()
                    .body(format!("AI response parse error: {}", e)),
            }
        }
        Ok(resp) => HttpResponse::BadGateway()
            .body(format!("AI service error: {}", resp.status())),
        Err(e) => HttpResponse::BadGateway()
            .body(format!("AI service unreachable: {}", e)),
    }
}

pub async fn get_team_morale(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let endpoint = if data.config.ai_use_local {
        &data.config.ai_local_endpoint
    } else {
        &data.config.ai_aws_endpoint
    };
    let url = format!("{}/morale/{}", endpoint.trim_end_matches('/'), team_id.into_inner());
    match data.http_client.get(&url).send().await {
        Ok(mut resp) if resp.status().is_success() => {
            HttpResponse::Ok().body(resp.text().await.unwrap_or_default())
        }
        Ok(resp) => HttpResponse::BadGateway()
            .body(format!("AI morale endpoint error: {}", resp.status())),
        Err(e) => HttpResponse::BadGateway()
            .body(format!("AI service unreachable: {}", e)),
    }
}
