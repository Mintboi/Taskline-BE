use uuid::Uuid;
use chrono::{Utc, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Task {
    #[serde(rename = "_id")]
    pub task_id: Uuid,
    pub team_id: String,
    pub title: String,
    pub description: String,
    pub priority: i32,
    pub assignee_id: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub team_id: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub assignee_id: Option<String>,
    pub status: Option<String>,
}
