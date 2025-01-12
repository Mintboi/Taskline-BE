use mongodb::{options::ClientOptions, Client, Database};

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
}
