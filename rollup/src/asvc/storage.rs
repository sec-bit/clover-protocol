use ckb_zkp::curve::PrimeField;
use ckb_zkp::math::{fft::EvaluationDomain, PairingEngine, Zero};
use ckb_zkp::scheme::asvc::{update_commit, verify_pos, Commitment, Parameters, Proof};
use ckb_zkp::scheme::r1cs::SynthesisError;
use std::collections::HashMap;
use std::ops::{Add, Neg, Sub};

use super::asvc::update_proofs;
use super::block::Block;
use super::transaction::{FullPubKey, Transaction, TxHash};

pub struct Storage<E: PairingEngine> {
    pub block_height: u32,
    pub omega: E::Fr,
    pub blocks: Vec<Block<E>>,
    pub pools: HashMap<TxHash, Transaction<E>>,
    pub proofs: Vec<Proof<E>>,
    pub params: Parameters<E>,
    pub commit: Commitment<E>,
    pub size: u32,
    pub next_user: u32,
    pub tmp_next_user: u32,
    pub balances: Vec<u32>,
    pub nonces: Vec<u32>,
    pub values: Vec<E::Fr>,
    pub full_pubkeys: Vec<FullPubKey<E>>,
    pub block_changes: HashMap<u32, HashMap<u32, (u32, u32, E::Fr, u32, u32, E::Fr)>>,
    pub block_remove: HashMap<u32, Vec<TxHash>>,
}

impl<E: PairingEngine> Storage<E> {
    pub fn init(params: Parameters<E>, commit: Commitment<E>, proofs: Vec<Proof<E>>) -> Self {
        let size = 1024 as usize;
        let domain = EvaluationDomain::<E::Fr>::new(size)
            .ok_or(SynthesisError::PolynomialDegreeTooLarge)
            .unwrap();

        Self {
            block_height: 0,
            omega: domain.group_gen,
            blocks: vec![],
            pools: HashMap::new(),
            proofs: proofs,
            params: params,
            commit: commit,
            size: size as u32,
            next_user: 0,
            tmp_next_user: 0,
            balances: vec![0; size],
            nonces: vec![0; size],
            values: vec![E::Fr::zero(); size],
            block_changes: HashMap::new(),
            block_remove: HashMap::new(),
            full_pubkeys: vec![],
        }
    }

    pub fn try_insert_tx(&mut self, tx: Transaction<E>) -> bool {
        let tx_hash = tx.hash();

        if !self.pools.contains_key(&tx_hash) {
            self.pools.insert(tx_hash, tx);
        }

        true
    }

    pub fn user_height_increment(&mut self) {
        self.next_user = self.next_user + 1;
    }

    pub fn tmp_user_height_increment(&mut self) {
        self.tmp_next_user = self.tmp_next_user + 1;
    }

    /// miner new block.
    pub fn create_block(&mut self) -> Option<Block<E>> {
        let block_height = self.block_height;
        let mut user_height = self.next_user;

        let n = self.size;
        let omega = self.omega;

        let mut new_commit = self.commit.clone();

        let mut changes = HashMap::<u32, (u32, u32, E::Fr, u32, u32, E::Fr)>::new();
        let mut removes = Vec::<TxHash>::new();
        let mut txs = Vec::<Transaction<E>>::new();

        let counter_changes = E::Fr::from_repr((2u64.pow(32)).into());

        for (tx_hash, tx) in self.pools.iter_mut() {
            match tx.tx_type {
                1 => {
                    // transfer
                    let cvalue = E::Fr::from_repr((tx.value as u64).into());
                    if changes.contains_key(&tx.i) {
                        let (balance, nonce, value, new_balance, new_nonce, new_value) =
                            changes[&tx.i];
                        if tx.nonce != new_nonce {
                            continue;
                        }
                        if tx.value < new_balance {
                            continue;
                        }
                        changes.insert(
                            tx.i,
                            (
                                balance,
                                nonce,
                                value,
                                new_balance - tx.value,
                                new_nonce + 1,
                                value.sub(&cvalue).add(&counter_changes),
                            ),
                        );
                    } else {
                        let proof = self.proofs[tx.i as usize].clone();
                        let balance = self.balances[tx.i as usize];
                        let nonce = self.balances[tx.i as usize];
                        let value = self.values[tx.i as usize];
                        if tx.nonce < nonce {
                            removes.push(tx_hash.clone());
                            continue;
                        }
                        if tx.nonce != nonce {
                            continue;
                        }
                        if tx.value < balance {
                            continue;
                        }

                        let result = verify_pos::<E>(
                            &self.params.verification_key,
                            &self.commit,
                            vec![value],
                            vec![tx.i],
                            &proof,
                            omega,
                        );
                        let result = match result {
                            Ok(result) => result,
                            Err(error) => {
                                //TODO
                                continue;
                            }
                        };
                        changes.insert(
                            tx.i,
                            (
                                balance,
                                nonce,
                                value,
                                balance - tx.value,
                                nonce + 1,
                                value.sub(&cvalue).add(&counter_changes),
                            ),
                        );
                    }
                    tx.proof = self.proofs[tx.i as usize].clone();

                    if changes.contains_key(&tx.j) {
                        let (balance, nonce, value, new_balance, new_nonce, new_value) =
                            changes[&tx.j];
                        changes.insert(
                            tx.j,
                            (
                                balance,
                                nonce,
                                value,
                                new_balance + tx.value,
                                new_nonce,
                                value.add(&cvalue),
                            ),
                        );
                    } else {
                        let balance = self.balances[tx.j as usize];
                        let nonce = self.balances[tx.j as usize];
                        let value = self.values[tx.j as usize];

                        changes.insert(
                            tx.j,
                            (
                                balance,
                                nonce,
                                value,
                                balance + tx.value,
                                nonce,
                                value.add(&cvalue),
                            ),
                        );
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        cvalue.neg().add(&counter_changes),
                        tx.i,
                        &self.params.proving_key.update_keys[tx.i as usize],
                        omega,
                        n as usize,
                    )
                    .unwrap();

                    new_commit = update_commit::<E>(
                        &new_commit,
                        cvalue,
                        tx.j,
                        &self.params.proving_key.update_keys[tx.j as usize],
                        omega,
                        n as usize,
                    )
                    .unwrap();
                }
                2 => {
                    // register
                    let mut value = self.values[tx.j as usize];
                    if changes.contains_key(&tx.i) {
                        // error
                    } else {
                        if user_height != tx.j {
                            continue;
                        }
                        value = tx.addr;
                        changes.insert(
                            tx.i,
                            (0, 0, E::Fr::zero(), 0, 1, value.add(&counter_changes)),
                        );
                        user_height = user_height + 1;
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        value.add(&counter_changes),
                        tx.i,
                        &self.params.proving_key.update_keys[tx.j as usize],
                        omega,
                        n as usize,
                    )
                    .unwrap();

                    tx.proof = self.proofs[tx.i as usize].clone();
                    self.full_pubkeys[tx.i as usize] = tx.full_pubkey.clone();
                }
                _ => todo!(),
            };
            txs.push(tx.clone());
            removes.push(tx_hash.clone());
        }

        let block = Block {
            block_height: block_height,
            commit: self.commit.clone(),
            new_commit: new_commit,
            txs: txs,
        };
        self.blocks.push(block.clone());

        self.block_changes.insert(block_height, changes);
        self.block_remove.insert(block_height, removes);

        Some(block)
    }

    /// handle when the block commit to L1.
    pub fn handle_block(&mut self, block: Block<E>) {
        let n = self.size;
        let omega = self.omega;

        self.block_changes.remove(&block.block_height);
        self.block_height = block.block_height;
        self.commit = block.new_commit;
        self.next_user = self.tmp_next_user;

        let removes = &self.block_remove[&block.block_height];
        for txhash in removes.iter() {
            self.pools.remove(txhash);
        }
        self.block_remove.remove(&block.block_height);

        let changes = &self.block_changes[&block.block_height];
        let mut cvalues = HashMap::<u32, E::Fr>::new();
        for (i, (balance, nonce, value, new_balance, new_nonce, new_value)) in changes {
            self.values[*i as usize] = *new_value;
            self.balances[*i as usize] = *new_balance;
            self.nonces[*i as usize] = *nonce;
            cvalues.insert(*i, new_value.sub(&value));
        }

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &block.commit,
            &mut self.proofs,
            &cvalues,
            n as usize,
        );

        self.block_changes.remove(&block.block_height);
    }

    /// if send to L1 failure, revert the block's txs.
    pub fn revert_block(&mut self, block: Block<E>) {
        self.block_remove.remove(&block.block_height);
        self.block_changes.remove(&block.block_height);
    }

    /// update local data from L1 for withdrawing and depositing
    pub fn update_block(&mut self, block: Block<E>) {
        let mut new_commit = self.commit.clone();
        let n = self.size;
        let omega = self.omega;
        let mut cvalues = HashMap::<u32, E::Fr>::new();

        for tx in block.txs.iter() {
            let value = E::Fr::from_repr((tx.value as u64).into());
            match tx.tx_type {
                1 => {
                    // deposit
                    self.balances[tx.i as usize] = self.balances[tx.i as usize] + tx.value;
                    self.values[tx.i as usize] = self.values[tx.i as usize].add(&value);
                    if cvalues.contains_key(&tx.i) {
                        cvalues.insert(tx.i, cvalues[&tx.i] + &value);
                    } else {
                        cvalues.insert(tx.i, value);
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        value,
                        tx.i,
                        &self.params.proving_key.update_keys[tx.i as usize],
                        omega,
                        n as usize,
                    )
                    .unwrap();
                }
                2 => {
                    // withdraw
                    self.balances[tx.i as usize] = self.balances[tx.i as usize] - tx.value;
                    self.values[tx.i as usize] = self.values[tx.i as usize].sub(&value);
                    if cvalues.contains_key(&tx.i) {
                        cvalues.insert(tx.i, cvalues[&tx.i] - &value);
                    } else {
                        cvalues.insert(tx.i, value.neg());
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        value.neg(),
                        tx.i,
                        &self.params.proving_key.update_keys[tx.i as usize],
                        omega,
                        n as usize,
                    )
                    .unwrap();
                }
                _ => todo!(),
            }
        }

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &self.commit,
            &mut self.proofs,
            &cvalues,
            n as usize,
        );

        self.block_height = block.block_height;
        self.commit = new_commit;
    }
}
