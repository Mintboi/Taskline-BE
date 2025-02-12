// File: chat_db.rs

use mongodb::{options::ClientOptions, Client, Database};
use mongodb::bson::{doc, Document};

pub struct MongoDB {
    pub client: Client,
    pub db: Database,
}

impl MongoDB {
    pub async fn init(uri: &str, db_name: &str) -> Self {
        let client_options = ClientOptions::parse(uri)
            .await
            .expect("Failed to parse MongoDB connection string");
        let client = Client::with_options(client_options).expect("Failed to initialize client");
        let db = client.database(db_name);
        MongoDB { client, db }
    }

    /// Returns a BSON filter document for the provided team_id.
    pub fn team_filter(&self, team_id: &str) -> Document {
        doc! { "team_id": team_id }
    }

    /// Merges an existing filter with a team filter.
    pub fn add_team_filter(&self, mut filter: Document, team_id: &str) -> Document {
        filter.insert("team_id", team_id);
        filter
    }

    /// Checks if the user belongs to the specified team.
    pub async fn check_user_team(&self, user_id: &str, team_id: &str) -> mongodb::error::Result<bool> {
        let collection = self.db.collection::<Document>("user_teams");
        let filter = doc! { "user_id": user_id, "team_id": team_id };
        let result = collection.find_one(filter).await?;
        Ok(result.is_some())
    }

    /// Checks if the user is a member of the project.
    pub async fn check_project_membership(&self, user_id: &str, project_id: &str) -> mongodb::error::Result<bool> {
        let collection = self.db.collection::<Document>("project_memberships");
        let filter = doc! { "user_id": user_id, "project_id": project_id };
        let result = collection.find_one(filter).await?;
        Ok(result.is_some())
    }

    /// Checks if the user is part of the chat.
    pub async fn check_chat_user(&self, user_id: &str, chat_id: &str) -> mongodb::error::Result<bool> {
        let collection = self.db.collection::<Document>("chat_users");
        let filter = doc! { "user_id": user_id, "chat_id": chat_id };
        let result = collection.find_one(filter).await?;
        Ok(result.is_some())
    }
}
