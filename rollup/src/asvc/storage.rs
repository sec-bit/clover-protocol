use ckb_zkp::curve::Field;
use ckb_zkp::gadgets::mimc;
use ckb_zkp::math::{fft::EvaluationDomain, BigInteger, PairingEngine, PrimeField, ToBytes, Zero};
use ckb_zkp::scheme::asvc::{
    aggregate_proofs, update_commit, verify_pos, Commitment, Parameters, Proof, UpdateKey,
};
use ckb_zkp::scheme::r1cs::SynthesisError;
use std::collections::HashMap;
use std::ops::{Add, Mul, Neg, Sub};

use super::asvc::update_proofs;

use asvc_rollup::block::Block;
use asvc_rollup::transaction::{u128_to_fr, FullPubKey, Transaction, TxHash, TxType, ACCOUNT_SIZE};

#[derive(Clone)]
pub struct block_storage<E: PairingEngine> {
    pub commit: Commitment<E>,
    // pub next_user: u32,
    pub balances: HashMap<u32, u128>,
    pub nonces: HashMap<u32, u32>,
    // pub txhashes: Vec<TxHash>,
}

pub struct Storage<E: PairingEngine> {
    pub block_height: u32,
    pub tmp_block_height: u32,
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
    // pub tmp_balances: Vec<u128>,

    pub nonces: Vec<u32>,
    pub tmp_nonces: Vec<u32>,

    pub tmp_storages: HashMap<u32, block_storage<E>>,

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

        Self {
            block_height: 0,
            tmp_block_height: 0,
            omega: domain.group_gen,
            blocks: vec![],
            pools: HashMap::new(),
            proofs: proofs,
            params: params,
            commit: commit,
            next_user: 0u32,
            tmp_next_user: 0u32,
            balances: vec![0u128; ACCOUNT_SIZE],
            // tmp_balances: vec![0u128; ACCOUNT_SIZE],
            nonces: vec![0u32; ACCOUNT_SIZE],
            tmp_nonces: vec![0u32; ACCOUNT_SIZE],
            full_pubkeys: full_pubkeys,
            tmp_storages: HashMap::new(),

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
        // self.tmp_balances[u as usize]
        self.balances[u as usize]
    }

    pub fn try_insert_tx(&mut self, tx: Transaction<E>) -> bool {
        let tx_hash = tx.hash();

        if !self.pools.contains_key(&tx_hash) {
            // match tx.tx_type {
            //     TxType::Transfer(from, to, amount) => {
            //         if amount > self.tmp_balances[from as usize] {
            //             return false;
            //         }

            //         self.tmp_balances[from as usize] -= amount;
            //         self.tmp_balances[to as usize] += amount;
            //         self.tmp_nonces[from as usize] += 1;
            //     }
            //     TxType::Register(account) => {
            //         self.tmp_next_user += 1;
            //         self.tmp_nonces[account as usize] = 1; // account first tx is register.
            //     }
            //     TxType::Deposit(_to, _amount) => {
            //         // not handle deposit
            //         return false;
            //     }
            //     TxType::Withdraw(_from, _amount) => {
            //         // not handle withdraw
            //         return false;
            //     }
            // }

            self.pools.insert(tx_hash, tx);
        }

        true
    }

    /// deposit & withdraw use when operate on L1, need build a block to change.
    pub fn build_block(&mut self, txs: Vec<Transaction<E>>) -> Option<Block<E>> {
        let n = ACCOUNT_SIZE;
        let omega = self.omega;

        let mut new_commit = self.commit.clone();

        let mut proofs = Vec::<Proof<E>>::new();
        let mut froms = vec![];

        //let nonce_offest_fr = E::Fr::one() >> 128;
        let mut repr = <E::Fr as PrimeField>::BigInt::from(1);
        for _ in 0..128 {
            // balance is u128
            repr.div2();
        }

        let nonce_offest_fr =
            <E::Fr as PrimeField>::from_repr(repr).mul(&E::Fr::from(2).pow(&[128]));

        let mut point_state = HashMap::<u32, (E::Fr, u32, u128, u32, i128)>::new();

        let mut storage = block_storage{
            commit: new_commit.clone(),
            balances: HashMap::new(),
            nonces: HashMap::new(),
        };

        let mut txlist = Vec::<Transaction<E>>::new();

        for tx in txs.iter(){
            let mut tx = tx.clone();
            match tx.tx_type {
                TxType::Transfer(from, to, amount) => {
                    tx.balance = self.balances[from as usize];
                    tx.proof = self.proofs[from as usize].clone();
                    let amount_fr: E::Fr = u128_to_fr::<E>(amount);
                    let from_upk = &self.params.proving_key.update_keys[from as usize];

                    if point_state.contains_key(&from) {
                        let (addr, nonce, balance, next_nonce, balance_change) = point_state[&from];
                        if next_nonce == 0 {
                            // no proof
                            if amount as i128 > tx.balance as i128 + balance_change {
                                continue
                            }

                            let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                            origin_proof_params += &(E::Fr::from_repr(
                                <E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1),
                            )
                            .mul(&E::Fr::from(2).pow(&[128])));
                            origin_proof_params += &(E::Fr::from_repr(
                                <E::Fr as PrimeField>::BigInt::from_u128(tx.balance),
                            ));

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
                            point_state.insert(
                                from,
                                (
                                    tx.addr,
                                    tx.nonce - 1,
                                    tx.balance,
                                    tx.nonce + 1,
                                    balance_change - amount as i128,
                                ),
                            );
                        } else {
                            // proved
                            if amount as i128 > tx.balance as i128 + balance_change {
                                continue;
                            }
                            if tx.nonce != next_nonce {
                                continue;
                            }
                            point_state.insert(
                                from,
                                (addr, nonce, balance, tx.nonce + 1, balance_change - amount as i128),
                            );
                        }
                    } else {
                        if amount > tx.balance {
                            continue;
                        }
                        let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                        origin_proof_params += &(E::Fr::from_repr(
                            <E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1),
                        )
                        .mul(&E::Fr::from(2).pow(&[128])));
                        origin_proof_params += &(E::Fr::from_repr(
                            <E::Fr as PrimeField>::BigInt::from_u128(tx.balance),
                        ));

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
                        point_state.insert(
                            from,
                            (tx.addr, tx.nonce - 1, tx.balance, tx.nonce + 1, 0 - amount  as i128),
                        );
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
                        point_state.insert(to,(addr, nonce, balance, next_nonce, balance_change + amount as i128));
                    } else {
                        point_state.insert(to, (E::Fr::zero(), 0, 0, 0, amount as i128));
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

                    if storage.nonces.contains_key(&from) {
                        let nonce = storage.nonces[&from];
                        storage.nonces.insert(from, nonce+1);
                    } else {
                        storage.nonces.insert(from, self.nonces[from as usize]+1);
                    }
                    if storage.balances.contains_key(&from) {
                        let balance = storage.balances[&from];
                        storage.balances.insert(from, balance - amount);
                    } else {
                        storage.balances.insert(from, self.balances[from as usize] - amount);
                    }
                    if storage.balances.contains_key(&to) {
                        let balance = storage.balances[&to];
                        storage.balances.insert(from, balance + amount);
                    } else {
                        storage.balances.insert(from, self.balances[to as usize] + amount);
                    }

                    txlist.push(tx);
                }
                TxType::Register(account) => {
                    tx.balance = self.balances[account as usize];
                    tx.proof = self.proofs[account as usize].clone();
                    let origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));

                    let from_upk = &self.params.proving_key.update_keys[account as usize];
                    if account >= self.next_user {
                        continue;
                    }
                    if point_state.contains_key(&account) {
                        continue;
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

                    self.tmp_next_user += 1;
                    storage.nonces.insert(account, 1);
                    storage.balances.insert(account, 0);
                    storage.commit = new_commit.clone();

                    txlist.push(tx);
                }
                TxType::Deposit(from, amount) => {
                    return None;
                }
                TxType::Withdraw(from, amount) => {
                    return None;
                }
            }
        }

        let proof = aggregate_proofs::<E>(froms, proofs, omega).unwrap();

        storage.commit = new_commit.clone();
        self.tmp_storages.insert(self.block_height + 1, storage);

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

        let mut proofs = Vec::<Proof<E>>::new();
        let mut froms = vec![];

        let mut point_state = HashMap::<u32, (E::Fr, u32, u128, u32, i128)>::new();

        let mut txlist = Vec::new();
        let mut tx = tx.clone();

        match tx.tx_type {
            TxType::Transfer(from, to, amount) => {
               return None;
            }
            TxType::Register(account) => {
                return None;
            }
            TxType::Deposit(from, amount) => {
                tx.balance = self.balances[from as usize];
                tx.proof = self.proofs[from as usize].clone();
                let from_upk = &self.params.proving_key.update_keys[from as usize];
                let amount_fr: E::Fr = u128_to_fr::<E>(amount);
                new_commit =
                    update_commit::<E>(&new_commit, amount_fr, from, from_upk, omega, n)
                        .unwrap();
                txlist.push(tx);
            }
            TxType::Withdraw(from, amount) => {
                tx.balance = self.balances[from as usize];
                tx.proof = self.proofs[from as usize].clone();
                let mut origin_proof_params = tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                origin_proof_params += &(E::Fr::from_repr(
                    <E::Fr as PrimeField>::BigInt::from(tx.nonce as u64 - 1),
                )
                .mul(&E::Fr::from(2).pow(&[128])));
                origin_proof_params +=
                    &(E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(tx.balance)));

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
                        return None;
                    }
                } else {
                    return None;
                }

                froms.push(tx.from());
                proofs.push(tx.proof.clone());

                new_commit =
                    update_commit::<E>(&new_commit, amount_fr.neg(), from, from_upk, omega, n)
                        .unwrap();

                txlist.push(tx);
            }
        }
 
        let proof = aggregate_proofs::<E>(froms, proofs, omega).unwrap();

        let block = Block {
            proof,
            block_height: self.block_height + 1,
            commit: self.commit.clone(),
            new_commit: new_commit,
            txs: txlist,
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
        // let mut txs = self.pools.clone();
        self.build_block(txs)
    }

    /// handle when the block commit to L1.
    pub fn handle_block(&mut self, block: Block<E>) {
        println!("HANDLE BLOCK: {}", block.block_height);
        let n = ACCOUNT_SIZE;

        self.block_height = block.block_height;
        self.commit = block.new_commit;

        let mut storage = self.tmp_storages[&block.block_height].clone();
        let mut cvalues = HashMap::<u32, E::Fr>::new();
        for (account, balance) in storage.balances.drain(){
            let cv = E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(balance - &self.balances[account as usize]));
            cvalues.insert(account, cv);
            self.balances[account as usize] = balance;
        }
        for (account, nonce) in storage.nonces.drain(){
            let mut cv = E::Fr::zero();
            if cvalues.contains_key(&account) {
                cv = cvalues[&account];
            }
            cv += &(
                E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from((nonce - &self.nonces[account as usize]) as u64)).mul(&E::Fr::from(2).pow(&[128]))
            );
            cvalues.insert(account, cv);
            self.nonces[account as usize] = nonce;
        }

        // change register full_pubkey
        for tx in block.txs {
            match tx.tx_type {
                TxType::Register(account) => {
                    let upk = self.params.proving_key.update_keys[account as usize].clone();

                    self.full_pubkeys[account as usize] = FullPubKey {
                        i: account,
                        update_key: upk,
                        tradition_pubkey: tx.pubkey.clone(),
                    };

                    let mut cv = E::Fr::zero();
                    if cvalues.contains_key(&account) {
                        cv = cvalues[&account];
                    }

                    cv += &tx.addr.mul(&E::Fr::from(2).pow(&[160]));
                    cvalues.insert(account, cv);
                }
                _ => continue,
            }
        }
        

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &block.commit,
            &mut self.proofs,
            &cvalues,
            n as usize,
        )
        .unwrap();

        self.block_height = block.block_height;
        let mut removes=Vec::new();
        for (height, storage) in self.tmp_storages.drain(){
            if height <=  block.block_height{
                removes.push(height)
                // self.tmp_storages.remove(&height);
            }
        }
        for i in removes.iter() {
            self.tmp_storages.remove(&i);
        }
    }

    pub fn sync_block(&mut self, block: Block<E>) {
        println!("HANDLE BLOCK: {}", block.block_height);
        if block.block_height <=  self.block_height{
            return;
        }

        if  block.block_height >  self.block_height + 1 {
            // error
            return;
        }
        if block.txs.len() != 1 {
            //error
            return;
        }
        
        let n = ACCOUNT_SIZE;
        let omega = self.omega;
        let mut new_commit = self.commit.clone();
        let tx = block.txs[0].clone();

        let mut cvalues = HashMap::<u32, E::Fr>::new();
        match tx.tx_type {
            TxType::Deposit(from, amount) => {
                self.balances[from as usize].add(amount);
                cvalues.insert(from as u32, E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(amount)));
            }
            TxType::Withdraw(from, amount) => {
                self.balances[from as usize].sub(amount);
                cvalues.insert(from as u32, E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(amount)).neg());
            }
            TxType::Register(account) => {
                return;
            }
            TxType::Transfer(from, to, amount) => {
                return;
            }
        }
        
        self.block_height = block.block_height;
        self.commit = block.new_commit;

        update_proofs::<E>(
            &self.params.proving_key.update_keys,
            &self.commit.clone(),
            &mut self.proofs,
            &cvalues,
            n as usize,
        ).unwrap();

    }

    /// if send to L1 failure, revert the block's txs.
    pub fn revert_block(&mut self, _block: Block<E>) {
        todo!()
    }
}
