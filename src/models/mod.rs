mod message;

use mongodb::bson::DateTime;
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

/// Represents a team in the system.
#[derive(Debug, Serialize, Deserialize)]
pub struct Team {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub name: String,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

/// Represents a user in the system.
#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub email: String,
    pub hashed_password: String,
    pub first_name: String,
    pub last_name: String,
    // Add any additional fields as needed (e.g., profile picture, status, etc.)
}

/// Roles for a user within a team.
#[derive(Debug, Serialize, Deserialize)]
pub enum UserTeamRole {
    Admin,
    Member,
}

/// Join table mapping a user to a team, with a role.
#[derive(Debug, Serialize, Deserialize)]
pub struct UserTeam {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub user_id: ObjectId,
    pub team_id: ObjectId,
    pub role: UserTeamRole,
}

/// Represents a project within a team.
#[derive(Debug, Serialize, Deserialize)]
pub struct Project {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub team_id: ObjectId,
    pub name: String,
    /// Optional description of the project.
    pub description: Option<String>,
}

/// Roles for a user within a project.
#[derive(Debug, Serialize, Deserialize)]
pub enum ProjectMembershipRole {
    Owner,
    Developer,
    Viewer,
}

/// Join table mapping a user to a project, with a role.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectMembership {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub user_id: ObjectId,
    pub project_id: ObjectId,
    pub role: ProjectMembershipRole,
}

/// Represents a chat room associated with a team.
#[derive(Debug, Serialize, Deserialize)]
pub struct Chat {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub team_id: ObjectId,
    pub name: String,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

/// Join table mapping users to a chat.
#[derive(Debug, Serialize, Deserialize)]
pub struct ChatUser {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub chat_id: ObjectId,
    pub user_id: ObjectId,
}

/// Represents a message sent in a chat.
#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    /// MongoDB document ID.
    #[serde(rename = "_id")]
    pub id: ObjectId,
    pub chat_id: ObjectId,
    pub sender_id: ObjectId,
    pub content: String,
    pub timestamp: DateTime,
    /// Optional list of attachment URLs.
    pub attachments: Option<Vec<String>>,
}
