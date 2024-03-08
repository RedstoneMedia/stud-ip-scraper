use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub display_name: String,
    pub username: String,
    pub avatar_src: Option<String>,
}