use ckb_zkp::math::PairingEngine;
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, Parameters, Proof, UpdateKey, VerificationKey,
};

use super::transaction::{FullPubKey, Transaction, TxHash};

#[derive(Clone)]
pub struct Block<E: PairingEngine> {
    pub txs: Vec<Transaction<E>>,
    pub block_height: u32,
    pub commit: Commitment<E>,
    pub proof: Proof<E>,
    pub new_commit: Commitment<E>,
}

impl<E: PairingEngine> Block<E> {
    pub fn to_hex(&self) -> String {
        todo!()
    }

    pub fn from_hex(s: &str) -> Result<Self, ()> {
        todo!()
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
