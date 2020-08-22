use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Account;

impl Account {
    pub fn generate() -> Self {
        Self
    }
}
