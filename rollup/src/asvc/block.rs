use ckb_zkp::math::PairingEngine;
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, FromByte, Parameters, Proof, ToBytes, UpdateKey,
    VerificationKey,
};

use super::transaction::{FullPubKey, Transaction, TxHash};

#[derive(Clone)]
pub struct Block<E: PairingEngine> {
    pub block_height: u32,
    pub commit: Commitment<E>,
    pub proof: Proof<E>,
    pub new_commit: Commitment<E>,
    pub txs: Vec<Transaction<E>>,
}

impl<E: PairingEngine> Block<E> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        self.block_height.write(&mut bytes).unwrap();
        self.commit.write(&mut bytes).unwrap();
        self.new_commit.write(&mut bytes).unwrap();
        self.proof.write(&mut bytes).unwrap();
        (self.txs.len() as u32).write(&mut bytes).unwrap();
        for tx in self.txs {
            tx.write(&mut bytes).unwrap();
        }
        bytes
    }

    pub fn from_bytes(s: &[u8]) -> Result<Self, ()> {}

    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    pub fn from_hex(s: &str) -> Result<Self, ()> {
        let v: Vec<u8> = hex::decode(s).map_err(|_| ())?;
        Self::from_bytes(&v)
    }

    pub fn verify(&self, vk: &VerificationKey<E>, omega: E::Fr) -> Result<bool, ()> {
        let mut proof_params = vec![];
        let mut froms = vec![];

        for tx in &self.txs {
            froms.push(tx.from());
            proof_params.push(tx.proof_param());
        }

        verify_pos::<E>(
            vk,
            &self.new_commit,
            proof_params,
            froms,
            &self.proof,
            omega,
        )
        .map_err(|_| ())
    }
}
