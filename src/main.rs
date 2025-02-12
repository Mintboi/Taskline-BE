// main.rs

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
    web, App, Error, HttpMessage, HttpResponse, HttpServer,
};
use env_logger::Env;
use futures::future::{ok, Ready};

use crate::auth::{login, signup};
use crate::team_management::{
    create_team, get_team_members, get_user_teams, invite_user,
    get_team, update_team, delete_team, remove_team_member,
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
// The "inner" service we wrap:
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
// The inner service's future must be 'static:
    S::Future: 'static,
// B is the typical response body type (e.g. BoxBody).
    B: MessageBody + 'static,
{
    // Force the middleware to always return BoxBody responses.
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    // The middleware transform will produce an AuthMiddleware<S> when built:
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    // Building the middleware is synchronous:
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
    // The middleware always returns responses with a boxed body.
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    // The future returned by `call`.
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    // Pass readiness checks through to the inner service.
    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    // The main entry point for each request.
    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        // 1) Attempt to read "Authorization: Bearer <token>" header:
        if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let token = auth_str.trim_start_matches("Bearer ").trim().to_string();
                    // 2) Verify token (replace with real JWT logic!)
                    match verify_token(&token) {
                        Ok(user_id) => {
                            // Insert user_id into request extensions.
                            req.extensions_mut().insert(user_id);
                        }
                        Err(e) => {
                            // On invalid token => immediately respond 401.
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

        // 3) If no/invalid token header, allow the request to proceed.
        let fut = self.service.call(req);
        // 4) Return a pinned async block that awaits the inner service call and converts its response body into a BoxBody.
        Box::pin(async move {
            let res = fut.await?;
            Ok(res.map_into_boxed_body())
        })
    }
}

/// A dummy function that "verifies" a token and returns Ok(user_id) or Err(...)
fn verify_token(_token: &str) -> Result<String, String> {
    // Replace with real JWT decoding:
    //   e.g., decode + validate signature, check expiry, then return claims.sub.
    // Return Err(...) if invalid / expired.
    Ok("dummy_user_id".to_string())
}

/// ---------------------------
/// 2) The main() function
/// ---------------------------
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load .env.
    dotenv::dotenv().ok();
    // Initialize logger.
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    // Load config and DB.
    let config = config::Config::from_env();
    let mongodb = Arc::new(chat_db::MongoDB::init(&config.mongo_uri, &config.database_name).await);
    let chat_server = chat_server::ChatServer::new(mongodb.clone()).start();
    // Allowed frontend origin from environment or default.
    let frontend_origin = env::var("FRONTEND_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    println!("Server running at http://0.0.0.0:8080");
    println!("Allowed CORS Origin: {}", frontend_origin);

    HttpServer::new(move || {
        // Configure CORS *inside* the factory closure so we don't clone it.
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

        // Build our application.
        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            // Insert the Authentication middleware to decode tokens.
            .wrap(Authentication)
            // Our shared AppState.
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
                        // Nest team-specific routes under /teams/{team_id}
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
                            // Project endpoints nested under a team.
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
        // etc...
    })
        .bind("0.0.0.0:8080")?
        .run()
        .await
}
// main.rs

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
    web, App, Error, HttpMessage, HttpResponse, HttpServer,
};
use env_logger::Env;
use futures::future::{ok, Ready};

use crate::auth::{login, signup};
use crate::team_management::{
    create_team, get_team_members, get_user_teams, invite_user,
    get_team, update_team, delete_team, remove_team_member,
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
// The "inner" service we wrap:
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
// The inner service's future must be 'static:
    S::Future: 'static,
// B is the typical response body type (e.g. BoxBody).
    B: MessageBody + 'static,
{
    // Force the middleware to always return BoxBody responses.
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    // The middleware transform will produce an AuthMiddleware<S> when built:
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    // Building the middleware is synchronous:
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
    // The middleware always returns responses with a boxed body.
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    // The future returned by `call`.
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    // Pass readiness checks through to the inner service.
    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    // The main entry point for each request.
    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        // 1) Attempt to read "Authorization: Bearer <token>" header:
        if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let token = auth_str.trim_start_matches("Bearer ").trim().to_string();
                    // 2) Verify token (replace with real JWT logic!)
                    match verify_token(&token) {
                        Ok(user_id) => {
                            // Insert user_id into request extensions.
                            req.extensions_mut().insert(user_id);
                        }
                        Err(e) => {
                            // On invalid token => immediately respond 401.
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

        // 3) If no/invalid token header, allow the request to proceed.
        let fut = self.service.call(req);
        // 4) Return a pinned async block that awaits the inner service call and converts its response body into a BoxBody.
        Box::pin(async move {
            let res = fut.await?;
            Ok(res.map_into_boxed_body())
        })
    }
}

/// A dummy function that "verifies" a token and returns Ok(user_id) or Err(...)
fn verify_token(_token: &str) -> Result<String, String> {
    // Replace with real JWT decoding:
    //   e.g., decode + validate signature, check expiry, then return claims.sub.
    // Return Err(...) if invalid / expired.
    Ok("dummy_user_id".to_string())
}

/// ---------------------------
/// 2) The main() function
/// ---------------------------
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load .env.
    dotenv::dotenv().ok();
    // Initialize logger.
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    // Load config and DB.
    let config = config::Config::from_env();
    let mongodb = Arc::new(chat_db::MongoDB::init(&config.mongo_uri, &config.database_name).await);
    let chat_server = chat_server::ChatServer::new(mongodb.clone()).start();
    // Allowed frontend origin from environment or default.
    let frontend_origin = env::var("FRONTEND_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());
    println!("Server running at http://0.0.0.0:8080");
    println!("Allowed CORS Origin: {}", frontend_origin);

    HttpServer::new(move || {
        // Configure CORS *inside* the factory closure so we don't clone it.
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

        // Build our application.
        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            // Insert the Authentication middleware to decode tokens.
            .wrap(Authentication)
            // Our shared AppState.
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
                        // Nest team-specific routes under /teams/{team_id}
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
                            // Project endpoints nested under a team.
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
        // etc...
    })
        .bind("0.0.0.0:8080")?
        .run()
        .await
}
