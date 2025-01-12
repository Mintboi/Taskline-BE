mod chat_server;
mod web_socket_server;
mod chat_db;
mod team_management;
mod tasks;
mod ai_endpoints;
mod auth;
mod config;
mod models;
mod knowledge_base;

use actix::{Actor, Addr};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web_actors::ws;
use actix_cors::Cors;
use std::sync::Arc;
use std::time::Instant;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use env_logger;
use uuid::Uuid;

use chat_server::{ChatServer, GetMessages};
use chat_db::MongoDB;
use web_socket_server::WebSocketConnection;
use config::Config;
use auth::protected;

struct AppState {
    chat_server: Addr<ChatServer>,
    mongodb: Arc<MongoDB>,
    config: Config,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let config = Config::from_env();

    let mongodb = Arc::new(MongoDB::init(&config.mongo_uri, &config.database_name).await);

    let chat_server = ChatServer::new(mongodb.clone()).start();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_header()
            .allow_any_method()
            .allow_any_origin()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(AppState {
                chat_server: chat_server.clone(),
                mongodb: mongodb.clone(),
                config: config.clone(),
            }))
            .route("/ws/", web::get().to(ws_index))
            // Public routes (no auth)
            .route("/signup", web::post().to(team_management::signup))
            .route("/login", web::post().to(team_management::login))
            // Protected routes (require JWT)
            .service(
                web::scope("")
                    .wrap(protected())
                    // Chat/Team routes:
                    .route("/chats/{user_id}", web::get().to(chat_server::get_user_chats))
                    .route("/messages/{user_id}", web::get().to(get_messages))
                    .route("/messages/create_chat", web::post().to(chat_server::create_chat))
                    .route("/create_team", web::post().to(team_management::create_team))
                    .route("/user_teams/{team_id}/members", web::get().to(team_management::get_team_members))
                    .route("/invite", web::post().to(team_management::invite_user))
                    .route("/get_team/{user_id}", web::get().to(team_management::get_user_teams))
                    // Task routes
                    .route("/tasks", web::post().to(tasks::create_task))
                    .route("/tasks/{team_id}", web::get().to(tasks::get_tasks_by_team))
                    .route("/tasks/{task_id}", web::put().to(tasks::update_task))
                    .route("/tasks/{task_id}", web::delete().to(tasks::delete_task))
                    // AI endpoints
                    .route("/ai/morale/{team_id}", web::get().to(ai_endpoints::get_team_morale))
                    .route("/ai/prioritize_tasks", web::post().to(ai_endpoints::prioritize_tasks))
            )
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let user_id = if let Some(uid) = req.uri().query() {
        let params: Vec<&str> = uid.split('=').collect();
        if params.len() == 2 && params[0] == "user_id" {
            Uuid::parse_str(params[1]).unwrap_or_else(|_| Uuid::new_v4())
        } else {
            Uuid::new_v4()
        }
    } else {
        Uuid::new_v4()
    };

    let ws = WebSocketConnection {
        id: user_id,
        hb: Instant::now(),
        addr: data.chat_server.clone(),
    };
    ws::start(ws, &req, stream)
}

#[derive(Deserialize)]
struct GetMessagesParams {
    user_id: String,
    chat_id: String,
    since: Option<String>,
}

async fn get_messages(
    data: web::Data<AppState>,
    params: web::Query<GetMessagesParams>,
) -> impl Responder {
    let user_id = match Uuid::parse_str(&params.user_id) {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest().body("Invalid user_id"),
    };

    let chat_id = match Uuid::parse_str(&params.chat_id) {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest().body("Invalid chat_id"),
    };

    let since = if let Some(since_str) = &params.since {
        match since_str.parse::<DateTime<Utc>>() {
            Ok(dt) => Some(dt),
            Err(_) => return HttpResponse::BadRequest().body("Invalid since parameter"),
        }
    } else {
        None
    };

    let res = data
        .chat_server
        .send(GetMessages {
            user_id,
            chat_id,
            since,
        })
        .await;

    match res {
        Ok(messages_response) => HttpResponse::Ok().json(messages_response),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
