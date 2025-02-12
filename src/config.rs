use std::env;
use mongodb::bson::doc;

#[derive(Clone)]
pub struct Config {
    pub mongo_uri: String,
    pub database_name: String,
    pub jwt_secret: String,
    /// Optional team identifier used for multi-tenancy.
    pub default_team_id: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();

        Self {
            mongo_uri: env::var("MONGO_URI").expect("MONGO_URI must be set"),
            database_name: env::var("DATABASE_NAME").unwrap_or_else(|_| "chat_db".to_string()),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            default_team_id: env::var("DEFAULT_TEAM_ID").ok(), // e.g., "team_123"
        }
    }

    /// Returns an optional MongoDB filter document to restrict queries to the configured team.
    pub fn team_filter(&self) -> Option<mongodb::bson::Document> {
        self.default_team_id.as_ref().map(|team_id| {
            doc! { "team_id": team_id }
        })
    }
}
