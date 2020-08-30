use ckb_zkp::math::{fft::EvaluationDomain, PairingEngine};
use ckb_zkp::scheme::asvc::{
    aggregate_proofs, update_commit, Commitment, Parameters, Proof, UpdateKey,
};
use ckb_zkp::scheme::r1cs::SynthesisError;
use std::collections::HashMap;

use asvc_rollup::block::{Block, CellUpks};
use asvc_rollup::transaction::{
    FullPubKey, PublicKey, SecretKey, Transaction, TxHash, TxType, ACCOUNT_SIZE,
};
use indexmap::IndexMap;

use super::asvc::update_proofs;

pub struct Storage<E: PairingEngine> {
    pub block_height: u32,
    pub tmp_block_height: u32,
    pub blocks: Vec<Block<E>>,
    pub pools: IndexMap<TxHash, Transaction<E>>,

    /// const params
    pub omega: E::Fr,
    pub params: Parameters<E>,
    pub cell_upks: CellUpks<E>,

    /// all accounts current proof.
    pub commit: Commitment<E>,
    pub proofs: Vec<Proof<E>>,

    pub full_pubkeys: Vec<FullPubKey<E>>,

    pub next_user: u32,
    pub tmp_next_user: u32,

    pub balances: Vec<u128>,
    pub tmp_balances: Vec<u128>,

    pub nonces: Vec<u32>,
    pub tmp_nonces: Vec<u32>,

    pub rollup_lock: String,
    pub rollup_dep: String,
    pub udt_lock: String, // use in withdraw
    pub my_udt: String,   // use in depost
    pub commit_cell: String,
    pub upk_cell: String,
    pub udt_cell: String,
    pub my_udt_amount: u128,
    pub total_udt_amount: u128,
}

impl<E: PairingEngine> Storage<E> {
    pub fn init(
        params: Parameters<E>,
        commit: Commitment<E>,
        proofs: Vec<Proof<E>>,
        full_pubkeys: Vec<FullPubKey<E>>,
    ) -> Self {
        let domain = EvaluationDomain::<E::Fr>::new(ACCOUNT_SIZE)
            .ok_or(SynthesisError::PolynomialDegreeTooLarge)
            .unwrap();

        let omega = domain.group_gen;
        let cell_upks = CellUpks {
            vk: params.verification_key.clone(),
            omega: omega,
            upks: params.proving_key.update_keys.clone(),
        };

        Self {
            block_height: 0,
            tmp_block_height: 0,
            omega: omega,
            cell_upks: cell_upks,
            blocks: vec![],
            pools: IndexMap::new(),
            proofs: proofs,
            params: params,
            commit: commit,
            next_user: 0u32,
            tmp_next_user: 0u32,
            balances: vec![0u128; ACCOUNT_SIZE],
            tmp_balances: vec![0u128; ACCOUNT_SIZE],
            nonces: vec![0u32; ACCOUNT_SIZE],
            tmp_nonces: vec![0u32; ACCOUNT_SIZE],
            full_pubkeys: full_pubkeys,

            rollup_lock: String::new(),
            rollup_dep: String::new(),
            udt_lock: String::new(),
            my_udt: String::new(),
            commit_cell: String::new(),
            upk_cell: String::new(),
            udt_cell: String::new(),
            my_udt_amount: 0,
            total_udt_amount: 0,
        }
    }

    pub fn next_nonce(&self, u: u32) -> u32 {
        self.tmp_nonces[u as usize]
    }

    pub fn next_user(&self) -> u32 {
        self.tmp_next_user
    }

    pub fn contains_users(&self, us: &[u32]) -> bool {
        for u in us {
            if *u >= self.next_user {
                return false;
            }
        }
        true
    }

    pub fn user_fpk(&self, u: u32) -> FullPubKey<E> {
        self.full_pubkeys[u as usize].clone()
    }

    pub fn user_upk(&self, u: u32) -> &UpdateKey<E> {
        &self.params.proving_key.update_keys[u as usize]
    }

    pub fn user_proof(&self, u: u32) -> Proof<E> {
        self.proofs[u as usize].clone()
    }

    pub fn pool_balance(&self, u: u32) -> u128 {
        self.tmp_balances[u as usize]
    }

    pub fn new_transfer(&self, from: u32, to: u32, amount: u128, sk: &SecretKey) -> Transaction<E> {
        Transaction::new_transfer(
            from,
            to,
            amount,
            self.user_fpk(from),
            self.next_nonce(from),
            // in one block, balance if current block balance, not tmp_balance
            self.balances[from as usize],
            self.user_proof(from),
            &sk,
        )
    }

    pub fn new_deposit(&self, from: u32, amount: u128, sk: &SecretKey) -> Transaction<E> {
        Transaction::new_deposit(
            from,
            amount,
            self.user_fpk(from),
            // use current block's nonce, not add it.
            self.nonces[from as usize],
            // in one block, balance if current block balance, not tmp_balance
            self.balances[from as usize],
            self.user_proof(from),
            &sk,
        )
    }

    pub fn new_withdraw(&self, from: u32, amount: u128, sk: &SecretKey) -> Transaction<E> {
        Transaction::new_withdraw(
            from,
            amount,
            self.user_fpk(from),
            // use current block's nonce, not add it.
            self.nonces[from as usize],
            // in one block, balance if current block balance, not tmp_balance
            self.balances[from as usize],
            self.user_proof(from),
            &sk,
        )
    }

    pub fn new_register(&self, from: u32, pk: PublicKey, sk: &SecretKey) -> Transaction<E> {
        let new_fpk = FullPubKey::<E> {
            i: from,
            update_key: self.user_upk(from).clone(),
            tradition_pubkey: pk,
        };

        Transaction::new_register(
            from,
            new_fpk,
            // when register his nonce must eq = 0
            0,
            // in one block, balance if current block balance, not tmp_balance
            self.balances[from as usize],
            self.user_proof(from),
            &sk,
        )
    }

    pub fn try_insert_tx(&mut self, tx: Transaction<E>) -> bool {
        let tx_hash = tx.hash();

        if !self.pools.contains_key(&tx_hash) {
            match tx.tx_type {
                TxType::Transfer(from, to, amount) => {
                    self.tmp_nonces[from as usize] += 1;
                    self.tmp_balances[from as usize] -= amount;
                    self.tmp_balances[to as usize] += amount;
                }
                TxType::Register(from) => {
                    self.tmp_next_user += 1;
                    self.tmp_nonces[from as usize] += 1;
                }
                TxType::Deposit(_to, _amount) => {
                    // not handle deposit
                    return false;
                }
                TxType::Withdraw(_from, _amount) => {
                    // not handle withdraw
                    return false;
                }
            }

            self.pools.insert(tx_hash, tx);
        }

        true
    }

    /// deposit & withdraw use when operate on L1, need build a block to change.
    pub fn build_block(&mut self, mut txs: Vec<Transaction<E>>) -> Option<Block<E>> {
        let n = ACCOUNT_SIZE;
        let omega = self.omega;

        let mut new_commit = self.commit.clone();

        let mut froms = IndexMap::new();
        let mut txlist: Vec<Transaction<E>> = vec![];

        println!("START BUILD BLOCK.... txs: {}", txs.len());

        loop {
            if txs.len() == 0 {
                break;
            }

            let tx = txs.remove(0);

            match tx.tx_type {
                TxType::Transfer(from, to, _amount) => {
                    let (from_amount, to_amount) = tx.delta_value();

                    // UPDATE FROM
                    new_commit = update_commit::<E>(
                        &new_commit,
                        from_amount,
                        from,
                        &self.user_upk(from),
                        omega,
                        n,
                    )
                    .expect("UPDATE TRANSFER FROM COMMIT FAILURE");

                    // UPDATE TO
                    new_commit = update_commit::<E>(
                        &new_commit,
                        to_amount,
                        to,
                        &self.user_upk(from),
                        omega,
                        n,
                    )
                    .expect("UPDATE TRANSFER TO COMMIT FAILURE");

                    if !froms.contains_key(&tx.from()) {
                        froms.insert(tx.from(), tx.proof.clone());
                    }
                }
                TxType::Register(account) => {
                    new_commit = update_commit::<E>(
                        &new_commit,
                        tx.delta_value().0,
                        account,
                        &self.user_upk(account),
                        omega,
                        n,
                    )
                    .expect("UPDATE REGISTER COMMIT FAILURE");

                    if !froms.contains_key(&tx.from()) {
                        froms.insert(tx.from(), tx.proof.clone());
                    }
                }
                TxType::Deposit(..) => {
                    panic!("NOOOOOP");
                }
                TxType::Withdraw(..) => {
                    panic!("NOOOOOP");
                }
            }

            txlist.push(tx);
        }

        let proof = aggregate_proofs::<E>(
            froms.keys().map(|v| *v).collect(),
            froms.values().map(|v| v.clone()).collect(),
            omega,
        )
        .expect("AGGREGATE ERROR");

        let block = Block {
            proof,
            block_height: self.block_height + 1,
            commit: self.commit.clone(),
            new_commit: new_commit,
            txs: txlist,
        };

        Some(block)
    }

    /// deposit & withdraw use when operate on L1, need build a block to change.
    pub fn build_block_by_user(&mut self, tx: Transaction<E>) -> Option<Block<E>> {
        let n = ACCOUNT_SIZE;
        let omega = self.omega;

        let mut new_commit = self.commit.clone();

        match tx.tx_type {
            TxType::Transfer(..) | TxType::Register(..) => {
                panic!("NOOOOP");
            }
            TxType::Deposit(from, _amount) => {
                new_commit = update_commit::<E>(
                    &new_commit,
                    tx.delta_value().0,
                    from,
                    &self.user_upk(from),
                    omega,
                    n,
                )
                .expect("UPDATE COMMIT DEPOSIT FAILURE");
            }
            TxType::Withdraw(from, _amount) => {
                new_commit = update_commit::<E>(
                    &new_commit,
                    tx.delta_value().0,
                    from,
                    &self.user_upk(from),
                    omega,
                    n,
                )
                .expect("UPDATE COMMIT DEPOSIT FAILURE");
            }
        }

        let proof = aggregate_proofs::<E>(vec![tx.from()], vec![tx.proof.clone()], omega)
            .expect("AGGREGATE PROOFS ERROR");

        let block = Block {
            proof,
            block_height: self.block_height + 1,
            commit: self.commit.clone(),
            new_commit: new_commit,
            txs: vec![tx],
        };

        Some(block)
    }

    /// miner new block.
    pub fn create_block(&mut self) -> Option<Block<E>> {
        if self.pools.len() == 0 {
            println!("miner block: no transactions.");
            return None;
        }

        let txs = self.pools.drain(..).map(|(_k, v)| v).collect();
        self.build_block(txs)
    }

    /// handle when the block commit to L1.
    pub fn handle_block(&mut self, block: Block<E>) {
        let n = ACCOUNT_SIZE;

        self.block_height = block.block_height;
        println!(
            "HANDLE BLOCK: block_height = {}, old commit = {}, new commit = {}",
            block.block_height, self.commit.commit, block.new_commit.commit
        );

        let mut cvalues = HashMap::new();

        // 1. update balance & fpk
        for tx in block.txs {
            match tx.tx_type {
                TxType::Deposit(from, amount) => {
                    self.balances[from as usize] += amount;
                    self.tmp_balances[from as usize] += amount;
                    let delta = tx.delta_value().0;

                    cvalues
                        .entry(from)
                        .and_modify(|f| *f += &delta)
                        .or_insert(delta);
                }
                TxType::Withdraw(from, amount) => {
                    self.balances[from as usize] -= amount;
                    self.tmp_balances[from as usize] -= amount;
                    let delta = tx.delta_value().0;

                    cvalues
                        .entry(from)
                        .and_modify(|f| *f += &delta)
                        .or_insert(delta);
                }
                TxType::Transfer(from, to, amount) => {
                    self.balances[from as usize] += amount;
                    self.balances[to as usize] -= amount;

                    let (from_delta, to_delta) = tx.delta_value();

                    cvalues
                        .entry(from)
                        .and_modify(|f| *f += &from_delta)
                        .or_insert(from_delta);

                    cvalues
                        .entry(to)
                        .and_modify(|f| *f += &to_delta)
                        .or_insert(to_delta);
                }
                TxType::Register(account) => {
                    self.full_pubkeys[account as usize] = FullPubKey {
                        i: account,
                        update_key: self.user_upk(account).clone(),
                        tradition_pubkey: tx.pubkey.clone(),
                    };
                    self.next_user += 1;

                    let delta = tx.delta_value().0;

                    cvalues
                        .entry(account)
                        .and_modify(|f| *f += &delta)
                        .or_insert(delta);
                }
            }
        }

        // 2. UPDATE COMMIT
        self.commit = block.new_commit;
        self.block_height = block.block_height;

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &self.commit,
            &mut self.proofs,
            &cvalues,
            n as usize,
        )
        .expect("UPDATE PROOFS FAILURE");

        println!("HANDLE BLOCK OVER");
    }

    /// if send to L1 failure, revert the block's txs.
    pub fn revert_block(&mut self, _block: Block<E>) {
        todo!()
    }
}
