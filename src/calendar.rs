use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use mongodb::bson::doc;
use serde::{Serialize, Deserialize};
use chrono::{Utc, DateTime};
use uuid::Uuid;
use log::{error};
use crate::app_state::AppState;
use crate::chat_server::RelaySignal;

#[derive(Debug, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub event_id: String,
    pub user_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub participants: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub participants: Vec<String>,
}

pub async fn create_event(
    req: HttpRequest,
    data: web::Data<AppState>,
    payload: web::Json<CreateEventRequest>,
) -> impl Responder {
    let current_user = req.extensions().get::<String>().cloned().unwrap_or_default();

    if payload.participants.iter().any(|p| p.is_empty()) {
        return HttpResponse::BadRequest().body("Invalid participant IDs provided.");
    }

    let new_event = CalendarEvent {
        event_id: Uuid::new_v4().to_string(),
        user_id: current_user.clone(),
        title: payload.title.clone(),
        start: payload.start,
        end: payload.end,
        participants: payload.participants.clone(),
        created_at: Utc::now(),
    };

    let collection = data.mongodb.db.collection::<CalendarEvent>("calendar_events");
    match collection.insert_one(&new_event).await {
        Ok(_) => {
            for participant in &payload.participants {
                let message = serde_json::json!({
                    "type": "calendar_invite",
                    "title": payload.title,
                    "start": payload.start,
                    "end": payload.end
                }).to_string();

                data.chat_server.do_send(RelaySignal {
                    user_id: participant.clone(),
                    chat_id: "".to_string(),
                    message,
                });
            }

            HttpResponse::Ok().json(new_event)
        }
        Err(e) => {
            error!("Error creating event: {}", e);
            HttpResponse::InternalServerError().body("Error creating event")
        }
    }
}

pub async fn get_user_events(
    path: web::Path<String>,
    data: web::Data<AppState>,
) -> impl Responder {
    let user_id = path.into_inner();
    let collection = data.mongodb.db.collection::<CalendarEvent>("calendar_events");
    let filter = doc! { "participants": user_id };

    match collection.find(filter).await {
        Ok(mut cursor) => {
            let mut events = Vec::new();
            while cursor.advance().await.unwrap_or(false) {
                if let Ok(event) = cursor.deserialize_current() {
                    events.push(event);
                }
            }
            HttpResponse::Ok().json(events)
        }
        Err(e) => {
            error!("Error fetching events: {}", e);
            HttpResponse::InternalServerError().body("Error fetching events")
        }
    }
}
