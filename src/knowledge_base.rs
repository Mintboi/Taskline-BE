use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use mongodb::bson::{doc, Uuid};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Serialize, Deserialize)]
pub struct Document {
    id: Uuid,
    team_id: String,
    title: String,
    content: String,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
}

// Create new document
pub async fn create_document(data: web::Data<AppState>, req: web::Json<Document>) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");
    let new_doc = Document {
        id: Uuid::new_v4(),
        team_id: req.team_id.clone(),
        title: req.title.clone(),
        content: req.content.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    match collection.insert_one(&new_doc).await {
        Ok(_) => HttpResponse::Ok().json(new_doc),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to save: {:?}", e)),
    }
}

// Fetch documents for a team
pub async fn get_team_documents(data: web::Data<AppState>, team_id: web::Path<String>) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");
    let cursor = collection.find(doc! { "team_id": team_id.as_str() }).await.unwrap();

    let docs: Vec<Document> = cursor.filter_map(|doc| doc.ok()).collect().await;
    HttpResponse::Ok().json(docs)
}
