use super::transaction::{Transaction, TxHash, FullPubKey};
use ckb_zkp::math::{PairingEngine};
use ckb_zkp::scheme::asvc::Commitment;

pub struct Block<E: PairingEngine>{
    pub txs: Vec<Transaction::<E>>,
    pub block_height: u32,
    pub commit: Commitment<E>,
    pub new_commit: Commitment<E>,
}

impl <E: PairingEngine> Block<E> {
    pub fn to_hex(&self) -> String {
        todo!()
    }

    pub fn from_hex(s: &str) -> Result<Self, ()> {
        todo!()
    }
}
