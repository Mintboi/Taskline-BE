mod auth;
mod team_management;
mod app_state;
mod config;
mod chat_server;
mod chat_db;
mod models;
mod web_socket_server;
mod project;
mod chat;
mod knowledge_base;
mod user_management; // Newly added module for user endpoints

use std::env;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::future::Future;
use std::pin::Pin;

use actix::Actor;
use actix_cors::Cors;
use actix_web::{
    body::{BoxBody, MessageBody},
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    http,
    middleware::Logger,
    web, App, Error, HttpMessage, HttpResponse, HttpServer, HttpRequest,
};
use env_logger::Env;
use futures::future::{ok, Ready};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::auth::{login, signup, Claims}; // We import Claims to decode the token properly
use crate::team_management::{
    create_team, get_team_members, get_user_teams, invite_user,
    get_team, update_team, delete_team, remove_team_member,
    accept_invitation, decline_invitation, delete_invitations,
};
use crate::project::{
    create_project, list_projects, get_project, update_project, delete_project,
};
use crate::app_state::AppState;

/// ---------------------------
/// 1) Define the Middleware
/// ---------------------------
#[derive(Debug)]
pub struct Authentication;

impl<S, B> Transform<S, ServiceRequest> for Authentication
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuthMiddleware { service })
    }
}

pub struct AuthMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for AuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        // Check for Authorization header
        if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let token = auth_str.trim_start_matches("Bearer ").trim().to_string();
                    match verify_token(&token) {
                        Ok(user_id) => {
                            // Insert the user_id from JWT into request extensions
                            req.extensions_mut().insert(user_id);
                        }
                        Err(e) => {
                            let (req_parts, _payload) = req.into_parts();
                            let resp = HttpResponse::Unauthorized()
                                .body(format!("Invalid token: {}", e))
                                .map_into_boxed_body();
                            let srv_resp = ServiceResponse::new(req_parts, resp);
                            return Box::pin(async move { Ok(srv_resp) });
                        }
                    }
                }
            }
        }

        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res.map_into_boxed_body())
        })
    }
}

/// Verify a JWT using the same secret that was used to create it.
/// This version decodes the JWT's `Claims` struct and returns `sub` (user_id) on success.
fn verify_token(token: &str) -> Result<String, String> {
    // In practice, you might pull from your AppState config instead of env:
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    ) {
        Ok(token_data) => Ok(token_data.claims.sub),
        Err(e) => Err(format!("Token decode error: {}", e)),
    }
}

/// ---------------------------
/// 2) The main() function
/// ---------------------------
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv::dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let config = config::Config::from_env();
    let mongodb = Arc::new(chat_db::MongoDB::init(&config.mongo_uri, &config.database_name).await);
    let chat_server = chat_server::ChatServer::new(mongodb.clone()).start();
    let frontend_origin = env::var("FRONTEND_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("Server running at http://0.0.0.0:8080");
    println!("Allowed CORS Origin: {}", frontend_origin);

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&frontend_origin)
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                http::header::CONTENT_TYPE,
                http::header::ACCEPT,
                http::header::AUTHORIZATION,
            ])
            .supports_credentials()
            .max_age(3600);

        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .wrap(Authentication)
            .app_data(web::Data::new(AppState {
                chat_server: chat_server.clone(),
                mongodb: mongodb.clone(),
                config: config.clone(),
            }))
            // Auth endpoints.
            .service(
                web::scope("/auth")
                    .route("/signup", web::post().to(signup))
                    .route("/login", web::post().to(login))
            )
            // Team endpoints.
            .service(
                web::scope("/teams")
                    .route("", web::post().to(create_team))
                    .service(
                        web::scope("/{team_id}")
                            .route("", web::get().to(get_team))
                            .route("", web::put().to(update_team))
                            .route("", web::delete().to(delete_team))
                            .service(
                                web::scope("/members")
                                    .route("", web::get().to(get_team_members))
                                    .route("", web::post().to(invite_user))
                                    .route("", web::delete().to(remove_team_member))
                            )
                            .service(
                                web::scope("/invitations")
                                    .route("/accept", web::post().to(accept_invitation))
                                    .route("/decline", web::post().to(decline_invitation))
                                    .route("", web::delete().to(delete_invitations))
                            )
                            .service(
                                web::scope("/projects")
                                    .route("", web::post().to(create_project))
                                    .route("", web::get().to(list_projects))
                                    .route("/{project_id}", web::get().to(get_project))
                                    .route("/{project_id}", web::put().to(update_project))
                                    .route("/{project_id}", web::delete().to(delete_project))
                            )
                    )
                    .route("/user_teams/{user_id}", web::get().to(get_user_teams))
            )
            // New Users endpoint for searching by email.
            .service(
                web::scope("/users")
                    .route("/find_user_email", web::get().to(user_management::find_user_email))
            )
    })
        .bind("0.0.0.0:8080")?
        .run()
        .await
}
