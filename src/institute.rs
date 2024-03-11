use serde::{Deserialize, Serialize};

/// Represents basic information about an institute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Institute {
    pub id: String,
    pub name: String,
}