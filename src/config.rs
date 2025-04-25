use std::env;
use mongodb::bson::doc;

#[derive(Clone)]
pub struct Config {
    pub mongo_uri: String,
    pub database_name: String,
    pub jwt_secret: String,
    pub default_team_id: Option<String>,
    pub ai_local_endpoint: String,
    pub ai_aws_endpoint: String,
    pub ai_use_local: bool,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv::dotenv().ok();
        let ai_use_local = env::var("AI_USE_LOCAL")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        Self {
            mongo_uri: env::var("MONGO_URI").expect("MONGO_URI must be set"),
            database_name: env::var("DATABASE_NAME").unwrap_or_else(|_| "chat_db".to_string()),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            default_team_id: env::var("DEFAULT_TEAM_ID").ok(),
            ai_local_endpoint: env::var("AI_LOCAL_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:9000".to_string()),
            ai_aws_endpoint: env::var("AI_AWS_ENDPOINT")
                .expect("AI_AWS_ENDPOINT must be set"),
            ai_use_local,
        }
    }

    pub fn team_filter(&self) -> Option<mongodb::bson::Document> {
        self.default_team_id.as_ref().map(|team_id| doc! { "team_id": team_id })
    }
}
