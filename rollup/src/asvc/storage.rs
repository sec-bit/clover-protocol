use ckb_zkp::gadgets::mimc;
use ckb_zkp::math::{fft::EvaluationDomain, BigInteger, PairingEngine, PrimeField, ToBytes, Zero};
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, Parameters, Proof, UpdateKey, aggregate_proofs,
};
use ckb_zkp::curve::Field;
use ckb_zkp::scheme::r1cs::SynthesisError;
use std::collections::HashMap;
use std::ops::{Add, Neg, Sub, Mul};

use super::asvc::update_proofs;

use asvc_rollup::block::Block;
use asvc_rollup::transaction::{u128_to_fr, FullPubKey, Transaction, TxHash, TxType, ACCOUNT_SIZE};

pub struct Storage<E: PairingEngine> {
    pub block_height: u32,
    pub blocks: Vec<Block<E>>,
    pub pools: HashMap<TxHash, Transaction<E>>,

    /// const params
    pub omega: E::Fr,
    pub params: Parameters<E>,

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
    pub udt_lock: String,
    pub commit_cell: String,
    pub upk_cell: String,
    pub udt_cell: String,
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

        Self {
            block_height: 0,
            omega: domain.group_gen,
            blocks: vec![],
            pools: HashMap::new(),
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
            commit_cell: String::new(),
            upk_cell: String::new(),
            udt_cell: String::new(),
        }
    }

    pub fn new_next_nonce(&self, u: u32) -> u32 {
        self.tmp_nonces[u as usize]
    }

    pub fn new_next_user(&self) -> (u32, UpdateKey<E>) {
        let account = self.tmp_next_user;
        (
            account,
            self.params.proving_key.update_keys[account as usize].clone(),
        )
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

    pub fn user_proof(&self, u: u32) -> Proof<E> {
        self.proofs[u as usize].clone()
    }

    pub fn user_balance(&self, u: u32) -> u128 {
        self.tmp_balances[u as usize]
    }

    pub fn try_insert_tx(&mut self, tx: Transaction<E>) -> bool {
        let tx_hash = tx.hash();

        if !self.pools.contains_key(&tx_hash) {
            match tx.tx_type {
                TxType::Transfer(from, to, amount) => {
                    if amount > self.tmp_balances[from as usize] {
                        return false;
                    }

                    self.tmp_balances[from as usize] -= amount;
                    self.tmp_balances[to as usize] += amount;
                    self.tmp_nonces[from as usize] += 1;
                }
                TxType::Register(account) => {
                    self.tmp_next_user += 1;
                    self.tmp_nonces[account as usize] = 1; // account first tx is register.
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
    pub fn build_block(&mut self, txs: Vec<Transaction<E>>) -> Option<Block<E>> {
        let n = ACCOUNT_SIZE;
        let omega = self.omega;

        let mut new_commit = self.commit.clone();

        let mut proofs = Vec::<Proof::<E>>::new();
        let mut froms = vec![];

        //let nonce_offest_fr = E::Fr::one() >> 128;
        let mut repr = <E::Fr as PrimeField>::BigInt::from(1);
        for _ in 0..128 {
            // balance is u128
            repr.div2();
        }

        let nonce_offest_fr = <E::Fr as PrimeField>::from_repr(repr).mul(&E::Fr::from(2).pow(&[128]));

        let mut point_state = HashMap::<u32, (E::Fr, u32, u128, u32, u128)>::new();

        for tx in &txs {
            match tx.tx_type {
                TxType::Transfer(from, to, amount) => {

                    let amount_fr: E::Fr = u128_to_fr::<E>(amount);
                    let from_upk = &self.params.proving_key.update_keys[from as usize];

                    if to >= self.next_user {
                        continue
                    }

                    if point_state.contains_key(&from) {
                        let (addr, nonce, balance, next_nonce, balance_change) = point_state[&from];
                        if next_nonce == 0 {
                            // no proof
                            if amount > tx.balance + balance_change{
                                continue
                            }
                            
                            let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                            origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1)).mul(&E::Fr::from(2).pow(&[128])));
                            origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(tx.balance)));

                            if let Ok(res) = verify_pos::<E>(
                                &self.params.verification_key,
                                &self.commit,
                                vec![origin_proof_params],
                                vec![from],
                                &tx.proof,
                                omega,
                            ) {
                                if !res {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                            froms.push(tx.from());
                            proofs.push(tx.proof.clone());
                            point_state.insert(from, (tx.addr, tx.nonce - 1, tx.balance, tx.nonce + 1, balance_change - amount));
                            
                        } else {
                            // proved
                            if amount > tx.balance + balance_change{
                                continue
                            }
                            if tx.nonce != next_nonce {
                                continue
                            }
                            point_state.insert(from, (addr, nonce, balance, tx.nonce + 1, balance_change - amount));
                        }
                    } else {
                        if amount > tx.balance {
                            continue
                        }
                        let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                        origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1)).mul(&E::Fr::from(2).pow(&[128])));
                        origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(tx.balance)));

                        if let Ok(res) = verify_pos::<E>(
                            &self.params.verification_key,
                            &self.commit,
                            vec![origin_proof_params],
                            vec![from],
                            &tx.proof,
                            omega,
                        ) {
                            if !res {
                                continue;
                            }
                        } else {
                            continue;
                        }
                        froms.push(tx.from());
                        proofs.push(tx.proof.clone());
                        point_state.insert(from, (tx.addr, tx.nonce - 1, tx.balance, tx.nonce + 1, 0 - amount));
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        amount_fr.neg().add(&nonce_offest_fr),
                        from,
                        from_upk,
                        omega,
                        n,
                    )
                    .unwrap();

                    if point_state.contains_key(&to) {
                        let (addr, nonce, balance, next_nonce, balance_change) = point_state[&to];
                        point_state.insert(to, (addr, nonce, balance, next_nonce, balance_change + amount));
                    } else {
                        point_state.insert(to, (E::Fr::zero(), 0, 0, 0, amount));
                    }

                    new_commit = update_commit::<E>(
                        &new_commit,
                        amount_fr,
                        to,
                        &self.full_pubkeys[to as usize].update_key,
                        omega,
                        n,
                    )
                    .unwrap();
                }
                TxType::Register(account) => {
                    let origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                    
                    let from_upk = &self.params.proving_key.update_keys[account as usize];
                    if account >= self.next_user {
                        continue
                    }
                    if point_state.contains_key(&account) {
                        continue
                    }
                    if let Ok(res) = verify_pos::<E>(
                        &self.params.verification_key,
                        &self.commit,
                        vec![tx.proof_param()],
                        vec![account],
                        &tx.proof,
                        omega,
                    ) {
                        if !res {
                            continue;
                        }
                    } else {
                        continue;
                    }
                    froms.push(tx.from());
                    proofs.push(tx.proof.clone());
                            
                    point_state.insert(account, (tx.addr, 0, 0, 1, 0));

                    new_commit = update_commit::<E>(
                        &new_commit,
                        origin_proof_params,
                        account,
                        &from_upk,
                        omega,
                        n as usize,
                    )
                    .unwrap();
                }
                TxType::Deposit(from, amount) => {
                    let from_upk = &self.params.proving_key.update_keys[from as usize];
                    let amount_fr: E::Fr = u128_to_fr::<E>(amount);
                    // TODO:
                    // froms.push(tx.from());
                    // proofs.push(tx.proof);
                    new_commit = update_commit::<E>(
                        &new_commit,
                        amount_fr,
                        from,
                        from_upk,
                        omega,
                        n,
                    )
                    .unwrap();
                }
                TxType::Withdraw(from, amount) => {
                    let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                    origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1)).mul(&E::Fr::from(2).pow(&[128])));
                    origin_proof_params += &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(tx.balance)));

                    let from_upk = &self.params.proving_key.update_keys[from as usize];
                    let amount_fr: E::Fr = u128_to_fr::<E>(amount);

                    if let Ok(res) = verify_pos::<E>(
                        &self.params.verification_key,
                        &self.commit,
                        vec![origin_proof_params],
                        vec![from],
                        &tx.proof,
                        omega,
                    ) {
                        if !res {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    froms.push(tx.from());
                    proofs.push(tx.proof.clone());
                            
                    new_commit = update_commit::<E>(
                        &new_commit,
                        amount_fr.neg(),
                        from,
                        from_upk,
                        omega,
                        n,
                    )
                    .unwrap();
                }
            }
        }

        let proof = aggregate_proofs::<E>(froms, proofs, omega).unwrap();


        let block = Block {
            proof,
            block_height: self.block_height,
            commit: self.commit.clone(),
            new_commit: new_commit,
            txs: txs,
        };

        Some(block)
    }

    /// miner new block.
    pub fn create_block(&mut self) -> Option<Block<E>> {
        if self.pools.len() == 0 {
            println!("miner block: no transactions.");
            return None;
        }

        let txs = self.pools.drain().map(|(_k, v)| v).collect();
        self.build_block(txs)
    }

    /// handle when the block commit to L1.
    pub fn handle_block(&mut self, block: Block<E>) {
        let n = ACCOUNT_SIZE;

        self.block_height = block.block_height;
        self.commit = block.new_commit;

        let mut olds = HashMap::<u32, E::Fr>::new();
        let mut cvalues = HashMap::<u32, E::Fr>::new();

        for (u, balance) in self.balances.iter().enumerate() {
            let mut bytes = Vec::new();
            let addr = self.full_pubkeys[u].addr();
            let nonce = self.nonces[u];

            addr.write(&mut bytes).unwrap();
            nonce.write(&mut bytes).unwrap();
            balance.to_le_bytes().write(&mut bytes).unwrap();

            olds.insert(u as u32, mimc::hash(&bytes));
        }

        // change register full_pubkey
        for tx in block.txs {
            match tx.tx_type {
                TxType::Register(account) => {
                    let upk = self.full_pubkeys[account as usize].update_key.clone();

                    self.full_pubkeys[account as usize] = FullPubKey {
                        i: account,
                        update_key: upk,
                        tradition_pubkey: tx.pubkey.clone(),
                    };
                }
                _ => continue,
            }
        }

        for (u, balance) in self.tmp_balances.iter().enumerate() {
            let mut bytes = Vec::new();
            let addr = self.full_pubkeys[u].addr();
            let nonce = self.tmp_nonces[u];

            addr.write(&mut bytes).unwrap();
            nonce.write(&mut bytes).unwrap();
            balance.to_le_bytes().write(&mut bytes).unwrap();

            let res = olds
                .get_mut(&(u as u32))
                .map(|i| mimc::hash::<E::Fr>(&bytes).sub(i))
                .unwrap();

            cvalues.insert(u as u32, res);
        }

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &block.commit,
            &mut self.proofs,
            &cvalues,
            n as usize,
        )
        .unwrap();

        self.next_user = self.tmp_next_user.clone();
        self.balances = self.tmp_balances.clone();
        self.nonces = self.tmp_nonces.clone();
    }

    /// if send to L1 failure, revert the block's txs.
    pub fn revert_block(&mut self, _block: Block<E>) {
        todo!()
    }
}
