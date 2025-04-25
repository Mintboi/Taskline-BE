//! Knowledge‑base REST handlers (stable id = Mongo _id → JSON id)

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use futures::stream::StreamExt;
use mongodb::bson::{doc, Uuid};
use serde::{Deserialize, Serialize};

use crate::AppState;

/* -------------------------------------------------------------------------- */
/* Models                                                                     */
/* -------------------------------------------------------------------------- */

/// Internal model – stored exactly as it lives in MongoDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Mongo primary key (kept as a UUID‑string for portability)
    #[serde(rename = "_id")]
    pub id: String,

    pub team_id: String,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What we expose to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct PublicDocument {
    pub id: String,
    pub team_id: String,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Document> for PublicDocument {
    fn from(d: Document) -> Self {
        Self {
            id: d.id,
            team_id: d.team_id,
            title: d.title,
            content: d.content,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

/* Client payloads                                                            */

#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub team_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    pub content: Option<String>,
}

/* -------------------------------------------------------------------------- */
/* Handlers                                                                   */
/* -------------------------------------------------------------------------- */

/// POST /knowledge_base
pub async fn create_document(
    data: web::Data<AppState>,
    req: web::Json<CreateDocumentRequest>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");

    let now = Utc::now();
    let new_doc = Document {
        id: Uuid::new().to_string(),
        team_id: req.team_id.clone(),
        title: req.title.clone(),
        content: req.content.clone(),
        created_at: now,
        updated_at: now,
    };

    match collection.insert_one(&new_doc).await {
        Ok(_) => HttpResponse::Ok().json(PublicDocument::from(new_doc)),
        Err(e) => HttpResponse::InternalServerError()
            .body(format!("Failed to save document: {e}")),
    }
}

/// GET /knowledge_base/{team_id}
pub async fn get_team_documents(
    data: web::Data<AppState>,
    team_id: web::Path<String>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");

    match collection
        .find(doc! { "team_id": team_id.as_str() })
        .await
    {
        Ok(mut cursor) => {
            let mut docs = Vec::<PublicDocument>::new();
            while let Some(doc) = cursor.next().await {
                if let Ok(d) = doc {
                    docs.push(PublicDocument::from(d));
                }
            }
            HttpResponse::Ok().json(docs)
        }
        Err(e) => HttpResponse::InternalServerError()
            .body(format!("Fetch failed: {e}")),
    }
}

/// GET /knowledge_base/doc/{id}
pub async fn get_document(
    data: web::Data<AppState>,
    id: web::Path<String>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");

    match collection.find_one(doc! { "_id": id.as_str() }).await {
        Ok(Some(doc)) => HttpResponse::Ok().json(PublicDocument::from(doc)),
        Ok(None)      => HttpResponse::NotFound().body("Document not found"),
        Err(e)        => HttpResponse::InternalServerError()
            .body(format!("Fetch failed: {e}")),
    }
}

/// PUT /knowledge_base/doc/{id}
pub async fn update_document(
    data: web::Data<AppState>,
    id: web::Path<String>,
    payload: web::Json<UpdateDocumentRequest>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");

    /* ------- build the $set object -------- */
    let mut set_doc = doc! { "updated_at": Utc::now().to_rfc3339() }; // store as RFC‑3339 string
    if let Some(t) = &payload.title   { set_doc.insert("title",   t); }
    if let Some(c) = &payload.content { set_doc.insert("content", c); }

    let filter = doc! { "_id": id.as_str() };
    let update = doc! { "$set": set_doc };

    /* ------- 1) perform the update -------- */
    match collection.update_one(filter.clone(), update).await {
        Ok(res) if res.matched_count == 0 => {
            return HttpResponse::NotFound().body("Document not found")
        }
        Ok(_) => { /* fall‑through */ }
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Update failed: {e}"))
        }
    }

    /* ------- 2) fetch the updated doc ----- */
    match collection.find_one(filter).await {
        Ok(Some(doc)) => HttpResponse::Ok().json(PublicDocument::from(doc)),
        Ok(None)      => HttpResponse::InternalServerError()
            .body("Document updated but could not be re‑fetched"),
        Err(e)        => HttpResponse::InternalServerError()
            .body(format!("Fetch after update failed: {e}")),
    }
}

/// DELETE /knowledge_base/doc/{id}
pub async fn delete_document(
    data: web::Data<AppState>,
    id: web::Path<String>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<Document>("knowledge_base");

    match collection
        .delete_one(doc! { "_id": id.as_str() })
         .await
    {
        Ok(res) if res.deleted_count == 1 => HttpResponse::NoContent().finish(),
        Ok(_)  => HttpResponse::NotFound().body("Document not found"),
        Err(e) => HttpResponse::InternalServerError()
            .body(format!("Delete failed: {e}")),
    }
}
