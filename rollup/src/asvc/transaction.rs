use serde::{Deserialize, Serialize};

use super::account::Account;

pub type TxHash = Vec<u8>;
pub type Amount = u64;

#[derive(Serialize, Deserialize)]
pub struct Transaction;

impl Default for Transaction {
    fn default() -> Self {
        Self
    }
}

impl Transaction {
    pub fn hash(&self) -> TxHash {
        vec![]
    }

    /// new transfer transaction.
    pub fn transfer(_from: Account, _to: Account, _amount: Amount) -> Self {
        todo!()
    }

    /// new deposit transaction.
    pub fn deposit() -> Self {
        todo!()
    }

    /// new withdraw transaction.
    pub fn withdraw() -> Self {
        todo!()
    }
}
