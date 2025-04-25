use crate::chat_server::ChatServer;
use crate::chat_db::MongoDB;
use crate::config::Config;
use actix::Addr;
use reqwest::Client;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub chat_server: Addr<ChatServer>,
    pub mongodb: Arc<MongoDB>,
    pub config: Config,
    pub http_client: Client,
}
