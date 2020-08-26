use ckb_zkp::gadgets::mimc;
use ckb_zkp::math::PairingEngine;
use ckb_zkp::math::ToBytes;
use ckb_zkp::scheme::asvc::{Proof, UpdateKey};
use serde::{Deserialize, Serialize};

use super::account::Account;

pub type TxHash = Vec<u8>;
pub type Amount = u64;

#[derive(Clone)]
pub struct FullPubKey<E: PairingEngine> {
    pub i: u32,
    pub updateKey: UpdateKey<E>,
    pub traditionPubKey: String,
}

impl<E: PairingEngine> FullPubKey<E> {
    pub fn hash(&self) -> E::Fr {
        let mut bytes = vec![];
        self.i.write(&mut bytes).unwrap();

        //todo!();
        //self.updateKey.write(&mut bytes).unwrap();
        //self.traditionPubKey.write(&mut bytes).unwrap();

        mimc::hash(&bytes)
    }
}

#[derive(Clone)]
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
    pub addr: E::Fr,
}

impl<E: PairingEngine> Default for Transaction<E> {
    fn default() -> Self {
        todo!()
    }
}

impl<E: PairingEngine> Transaction<E> {
    pub fn hash(&self) -> TxHash {
        vec![]
    }

    pub fn hash_string(&self) -> String {
        "0x000000".to_owned()
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

#[derive(Serialize, Deserialize)]
pub struct RawTransaction {
    pub tx_type: u8,
    pub full_pubkey: String,
    pub i: u32,
    pub j: u32,
    pub j_updatekey: String,
    pub value: u32,
    pub nonce: u32,
    pub proof: String,
    pub balance: u32,
    pub addr: String,
}

impl RawTransaction {
    pub fn to_tx<E: PairingEngine>(&self) -> Result<Transaction<E>, ()> {
        todo!()
    }

    pub fn from_tx<E: PairingEngine>(tx: &Transaction<E>) -> Result<RawTransaction, ()> {
        todo!()
    }
}
