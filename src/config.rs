use dotenv::dotenv;
use std::env;

pub struct Config {
    pub mongo_uri: String,
    pub database_name: String,
    pub jwt_secret: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().ok();
        let mongo_uri = env::var("MONGO_URI").expect("MONGO_URI must be set");
        let database_name = env::var("DATABASE_NAME").expect("DATABASE_NAME must be set");
        let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

        Config {
            mongo_uri,
            database_name,
            jwt_secret,
        }
    }
}
