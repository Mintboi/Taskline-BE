use actix_web::{web, HttpResponse, Responder, HttpMessage, dev::ServiceRequest, dev::ServiceResponse, Error};
use actix_web_lab::middleware::from_fn;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{Utc, Duration};
use jsonwebtoken::{encode, decode, EncodingKey, DecodingKey, Header, Validation};
use mongodb::bson::{doc, Uuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid as UuidV4;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserDocument {
    pub id: Uuid,
    pub user_id: String,
    pub title: String,
    pub content: String,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct SignupInfo {
    pub username: String,
    pub password: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct LoginInfo {
    pub username: String,
    pub password: String,
}

// JWT Creation
pub fn create_jwt(user_id: &str, secret: &str) -> String {
    let expiration = Utc::now() + Duration::hours(24);
    let claims = Claims {
        sub: user_id.to_string(),
        exp: expiration.timestamp() as usize,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref())).unwrap()
}

// JWT Validation
pub fn validate_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

// Middleware for Authentication
pub async fn auth_middleware(
    req: ServiceRequest,
    srv: actix_web::dev::Service<ServiceRequest>,
) -> Result<ServiceResponse, Error> {
    if let Some(header_value) = req.headers().get("Authorization") {
        if let Ok(auth_str) = header_value.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = auth_str.trim_start_matches("Bearer ");
                let secret = req.app_data::<AppState>().unwrap().config.jwt_secret.clone();
                if validate_jwt(token, &secret).is_ok() {
                    return srv.call(req).await;
                }
            }
        }
    }
    Err(actix_web::error::ErrorUnauthorized("Unauthorized"))
}

pub fn protected() -> actix_web_lab::middleware::FromFn {
    from_fn(auth_middleware)
}

// Signup Endpoint
pub async fn signup(
    data: web::Data<AppState>,
    signup_info: web::Json<SignupInfo>,
) -> impl Responder {
    let users_collection = data.mongodb.db.collection::<User>("users");
    let hashed_password = match hash(&signup_info.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => return HttpResponse::InternalServerError().body("Error hashing password"),
    };

    let new_user = User {
        user_id: UuidV4::new_v4().to_string(),
        username: signup_info.username.clone(),
        email: signup_info.email.clone(),
        password: hashed_password,
    };

    match users_collection.insert_one(&new_user, None).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "status": "User created" })),
        Err(e) => HttpResponse::InternalServerError().body(format!("Error: {:?}", e)),
    }
}

// Login Endpoint
pub async fn login(
    data: web::Data<AppState>,
    login_info: web::Json<LoginInfo>,
) -> impl Responder {
    let users_collection = data.mongodb.db.collection::<User>("users");
    let user_doc = users_collection
        .find_one(doc! { "username": &login_info.username }, None)
        .await;

    match user_doc {
        Ok(Some(user)) => {
            if verify(&login_info.password, &user.password).unwrap_or(false) {
                let token = create_jwt(&user.user_id, &data.config.jwt_secret);
                HttpResponse::Ok().json(serde_json::json!({ "token": token, "user_id": user.user_id }))
            } else {
                HttpResponse::Unauthorized().body("Invalid credentials")
            }
        }
        Ok(None) => HttpResponse::Unauthorized().body("User not found"),
        Err(_) => HttpResponse::InternalServerError().body("Error logging in"),
    }
}

// Add User Document
pub async fn add_user_document(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
    doc_info: web::Json<UserDocument>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<UserDocument>("user_documents");
    let new_doc = UserDocument {
        id: UuidV4::new_v4(),
        user_id: user_id.into_inner(),
        title: doc_info.title.clone(),
        content: doc_info.content.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    match collection.insert_one(&new_doc, None).await {
        Ok(_) => HttpResponse::Ok().json(new_doc),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to save: {:?}", e)),
    }
}

// Fetch User Documents
pub async fn get_user_documents(
    data: web::Data<AppState>,
    user_id: web::Path<String>,
) -> impl Responder {
    let collection = data.mongodb.db.collection::<UserDocument>("user_documents");
    let cursor = collection
        .find(doc! { "user_id": user_id.into_inner() }, None)
        .await
        .unwrap();

    let docs: Vec<UserDocument> = cursor.filter_map(|doc| doc.ok()).collect().await;
    HttpResponse::Ok().json(docs)
}
