use ckb_zkp::math::{FromBytes, PairingEngine, ToBytes};
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, Proof, UpdateKey, VerificationKey,
};

use crate::transaction::{Transaction, ACCOUNT_SIZE};
use crate::{String, Vec};

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
        let mut bytes = Vec::new();
        self.block_height.write(&mut bytes).unwrap();
        self.commit.write(&mut bytes).unwrap();
        self.new_commit.write(&mut bytes).unwrap();
        self.proof.write(&mut bytes).unwrap();
        (self.txs.len() as u32).write(&mut bytes).unwrap();
        for tx in &self.txs {
            tx.write(&mut bytes).unwrap();
        }
        bytes
    }

    pub fn from_bytes(mut s: &[u8]) -> Result<Self, ()> {
        let block_height = u32::read(&mut s).map_err(|_| ())?;

        let commit = Commitment::read(&mut s).map_err(|_| ())?;
        let new_commit = Commitment::read(&mut s).map_err(|_| ())?;
        let proof = Proof::read(&mut s).map_err(|_| ())?;
        let n = u32::read(&mut s).map_err(|_| ())?;
        let mut txs = Vec::new();
        for _ in 0..n {
            txs.push(Transaction::read(&mut s).map_err(|_| ())?);
        }

        Ok(Self {
            block_height,
            commit,
            new_commit,
            proof,
            txs,
        })
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    pub fn from_hex(s: &str) -> Result<Self, ()> {
        let v: Vec<u8> = hex::decode(s).map_err(|_| ())?;
        Self::from_bytes(&v[..])
    }

    pub fn verify(
        &self,
        vk: &VerificationKey<E>,
        omega: E::Fr,
        upks: &Vec<UpdateKey<E>>,
    ) -> Result<bool, ()> {
        if upks.len() != ACCOUNT_SIZE {
            return Err(());
        }

        let mut proof_params = Vec::new();
        let mut froms = Vec::new();

        let mut tmp_commit = self.commit.clone();

        for tx in &self.txs {
            let from = tx.from();
            let delta = tx.proof_param();

            froms.push(from);
            proof_params.push(delta);

            tmp_commit = update_commit(
                &tmp_commit,
                delta,
                from,
                &upks[from as usize],
                omega,
                ACCOUNT_SIZE,
            )
            .map_err(|_| ())?;
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
