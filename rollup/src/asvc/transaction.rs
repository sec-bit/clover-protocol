use serde::{Deserialize, Serialize};
use ckb_zkp::scheme::asvc::{UpdateKey, Proof};
use ckb_zkp::math::{PairingEngine};


use super::account::Account;

pub type TxHash = Vec<u8>;
pub type Amount = u64;

#[derive(Serialize, Deserialize)]
pub struct FullPubKey<E: PairingEngine> {
    pub i: u32,
    pub updateKey: UpdateKey<E>,
    pub traditionPubKey: String,
}

#[derive(Serialize, Deserialize)]
pub struct Transaction<E: PairingEngine> {
    pub tx_type: u8,
    pub full_pubkey: FullPubKey<E>,
    pub i: u32,
    pub j: u32,
    pub j_updatekey: UpdateKey<E>,
    pub value: u32,
    pub nonce: u32,
    pub proof: Proof<E>,
    pub balance: u32,
}

impl <E: PairingEngine> Default for Transaction<E> {
    fn default() -> Self {
        Self
    }
}

impl <E: PairingEngine> Transaction<E> {
    pub fn hash(&self) -> TxHash {
        vec![]
    }

    /// new transfer transaction.
    pub fn transfer(_from: Account, _to: Account, _amount: Amount) -> Self {
        todo!()
    }

    /// new deposit transaction.
    pub fn register() -> Self {
        todo!()
    }
}
