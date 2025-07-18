// src/main.rs

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
mod user_management;
mod board;
mod ticket;
mod calendar;
mod ai_endpoints;
mod dashboard_data;

use std::env;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::future::Future;
use std::pin::Pin;

use actix::Actor;
use actix_cors::Cors;
use actix_web::{body::{BoxBody, MessageBody}, dev::{Service, ServiceRequest, ServiceResponse, Transform}, http, middleware::Logger, web, App, Error, HttpMessage, HttpResponse, HttpServer};
use env_logger::Env;
use futures::future::{ok, Ready};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::user_management::{get_working_hours, set_working_hours};
use crate::calendar::{create_event, get_user_events};
use crate::auth::{login, signup, Claims};
use crate::team_management::{
    create_team, get_team_members, get_user_teams, invite_user,
    get_team, update_team, delete_team, remove_team_member,
    accept_invitation, decline_invitation, delete_invitations, get_pending_invitations,
};
use crate::project::{
    create_project, list_projects, get_project, update_project, delete_project,add_user_to_project
};
use crate::app_state::AppState;
use crate::chat::{
    get_user_chats, create_chat, search_chats, delete_chat,
    get_single_chat, update_chat, create_message, get_messages,
};
use crate::user_management::{find_user_email, get_user_by_id};
use crate::web_socket_server::ws_index;
use crate::board::{
    list_boards, create_board, update_board, delete_board, add_user_to_board,
};
use crate::ticket::{
    create_ticket, list_tickets, get_ticket, update_ticket, delete_ticket,
};
use crate::knowledge_base::{
    create_document, delete_document, get_team_documents, update_document,
};
use crate::dashboard_data::{get_dashboard_data, upsert_dashboard_data};

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
        if let Some(auth_header) = req.headers().get(http::header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let token = auth_str.trim_start_matches("Bearer ").trim().to_string();
                    match verify_token(&token) {
                        Ok(user_id) => {
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

fn verify_token(token: &str) -> Result<String, String> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    ) {
        Ok(token_data) => Ok(token_data.claims.sub),
        Err(e) => Err(format!("Token decode error: {}", e)),
    }
}

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
            .allowed_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
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
                http_client: Default::default(),
            }))
            // auth
            .service(
                web::scope("/auth")
                    .route("/signup", web::post().to(signup))
                    .route("/login", web::post().to(login))
            )
            // teams & related
            .service(
                web::scope("/teams")
                    .route("/user_teams/{user_id}", web::get().to(get_user_teams))
                    .route("/user_invitations/{user_id}", web::get().to(get_pending_invitations))
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
                                    .route("/{project_id}/members", web::post().to(add_user_to_project))
                                    .service(
                                        web::scope("/{project_id}/boards")
                                            .route("", web::get().to(list_boards))
                                            .route("", web::post().to(create_board))
                                            .route("/{board_id}", web::put().to(update_board))
                                            .route("/{board_id}", web::delete().to(delete_board))
                                            .route("/{board_id}/members", web::post().to(add_user_to_board))
                                    )
                                    .service(
                                        web::scope("/{project_id}/tickets")
                                            .route("", web::get().to(list_tickets))
                                            .route("", web::post().to(create_ticket))
                                            .route("/{ticket_id}", web::get().to(get_ticket))
                                            .route("/{ticket_id}", web::put().to(update_ticket))
                                            .route("/{ticket_id}", web::delete().to(delete_ticket))
                                    )
                            )
                    )
            )
            //TEAM-DATA
            .service(
                web::scope("/team-data")
                    .route("/{team_id}", web::get().to(get_dashboard_data))
                    .route("/{team_id}", web::put().to(upsert_dashboard_data))
            )
            // chats & messages
            .service(
                web::scope("/chats")
                    .route("/{user_id}", web::get().to(get_user_chats))
                    .route("", web::post().to(create_chat))
                    .route("/search/{user_id}", web::get().to(search_chats))
                    .route("/{chat_id}", web::patch().to(update_chat))
                    .route("/{chat_id}", web::delete().to(delete_chat))
                    .route("/get/{chat_id}", web::get().to(get_single_chat))
            )
            .service(
                web::scope("/messages")
                    .route("/{chat_id}", web::get().to(get_messages))
                    .route("/{chat_id}", web::post().to(create_message))
            )

            // users
            .service(
                web::scope("/users")
                    .route("/find_user_email", web::get().to(find_user_email))
                    .route("/get/{id}", web::get().to(get_user_by_id))
                    .route("/working-hours", web::get().to(get_working_hours))
                    .route("/working-hours", web::post().to(set_working_hours))
            )

            // websocket
            .service(web::resource("/ws").route(web::get().to(ws_index)))

            // calendar
            .service(
                web::scope("/calendar")
                    .route("/events", web::post().to(create_event))
                    .route("/events/{user_id}", web::get().to(get_user_events))
            )

            // knowledge base
            .service(
                web::scope("/knowledge_base")
                    .route("", web::post().to(create_document))
                    .route("/{team_id}", web::get().to(get_team_documents))
                    .route("/{doc_id}", web::put().to(update_document))
                    .route("/{doc_id}", web::delete().to(delete_document))
            )
    })
        .bind(("0.0.0.0", 8080))?
        .run()
        .await
}
