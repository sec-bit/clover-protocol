use async_std::{
    sync::{Arc, Mutex},
    task,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tide::{Error, Request, StatusCode};

use ckb_zkp::curve::bn_256::Bn_256;
use ckb_zkp::math::{PairingEngine, ToBytes};

mod asvc;
mod storage;

use asvc::initialize_asvc;
use storage::Storage;

use asvc_rollup::block::Block;
use asvc_rollup::transaction::{FullPubKey, PublicKey, SecretKey, Transaction, ACCOUNT_SIZE};
use ckb_rpc::{
    deploy_contract, init_state, listen_blocks, send_block, send_deposit, send_withdraw,
};

/// listening task.
async fn listen_contracts<E: PairingEngine>(
    storage: Arc<Mutex<Storage<E>>>,
) -> Result<(), std::io::Error> {
    let mut l1_block_height = 0;

    loop {
        // 10s to read lastest block to check if block has deposit tx.
        task::sleep(Duration::from_secs(1000)).await;
        println!(
            "Listen Task: start read block's txs. Current block height: {}",
            l1_block_height
        );

        let rollup_lock = &storage.lock().await.rollup_lock;

        if let Ok((blocks, new_height)) = listen_blocks(l1_block_height, rollup_lock).await {
            for (bytes, new_commit, new_upk, is_new_udt) in blocks {
                if let Ok(block) = Block::from_bytes(&bytes[..]) {
                    storage.lock().await.handle_block(block);

                    storage.lock().await.commit_cell = new_commit;
                    storage.lock().await.upk_cell = new_upk;
                    if let Some((new_udt, amount)) = is_new_udt {
                        storage.lock().await.udt_cell = new_udt;
                        storage.lock().await.total_udt_amount = amount;
                    }
                }
            }

            l1_block_height = new_height;
        }

        println!(
            "Listen Task: end read block's txs. Current block height: {}",
            l1_block_height
        );
    }
}

/// Miner task.
async fn miner<E: PairingEngine>(storage: Arc<Mutex<Storage<E>>>) -> Result<(), std::io::Error> {
    let vk = storage.lock().await.params.verification_key.clone();
    let omega = storage.lock().await.omega.clone();

    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        if let Some(block) = storage.lock().await.create_block() {
            println!(
                "Block verify is: {:?}",
                block.verify(
                    &vk,
                    omega,
                    &storage.lock().await.params.proving_key.update_keys
                )
            );
            let rollup_hash: &String = &storage.lock().await.rollup_lock;
            let rollup_dep_hash: &String = &storage.lock().await.rollup_dep;
            let pre_commit_hash: &String = &storage.lock().await.commit_cell;
            let pre_upk_hash: &String = &storage.lock().await.upk_cell;
            let block_bytes: Vec<u8> = block.to_bytes();

            let mut upks = vec![];
            for upk in &storage.lock().await.params.proving_key.update_keys {
                let mut tmp_bytes = vec![];
                upk.write(&mut tmp_bytes).unwrap();
                upks.push(tmp_bytes);
            }

            if let Ok((new_commit_cell, new_upk_cell, tx_id)) = send_block(
                rollup_hash,
                rollup_dep_hash,
                pre_commit_hash,
                pre_upk_hash,
                block_bytes,
                upks,
            )
            .await
            {
                storage.lock().await.commit_cell = new_commit_cell;
                storage.lock().await.upk_cell = new_upk_cell;

                println!("block send L1 is success: tx: {}", tx_id);
                storage.lock().await.handle_block(block);
            } else {
                storage.lock().await.revert_block(block);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterRequest {
    pub pubkey: String,
    pub psk: String,
}

async fn register<E: PairingEngine>(
    mut req: Request<Arc<Mutex<Storage<E>>>>,
) -> Result<String, Error> {
    let params: RegisterRequest = req.body_json().await?;

    let (pubkey, psk) = (
        PublicKey::from_hex(&params.pubkey).unwrap(),
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let (account, upk) = req.state().lock().await.new_next_user();
    println!("new next user: {}", account);
    let proof = req.state().lock().await.user_proof(account).clone();

    let fpk = FullPubKey::<E> {
        i: account,
        update_key: upk,
        tradition_pubkey: pubkey.clone(),
    };

    let tx = Transaction::<E>::new_register(account, fpk, 0, 0, proof, &psk);

    let tx_id = tx.id();

    if req.state().lock().await.try_insert_tx(tx) {
        Ok(tx_id)
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct DepositRequest {
    pub to: u32,
    pub amount: u128,
    pub psk: String,
}

/// wallet deposit api. build tx and send to ckb.
async fn deposit<E: PairingEngine>(
    mut req: Request<Arc<Mutex<Storage<E>>>>,
) -> Result<String, Error> {
    let params: DepositRequest = req.body_json().await?;
    let (from, amount, sk) = (
        params.to,
        params.amount,
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    if !req.state().lock().await.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let fpk = req.state().lock().await.user_fpk(from);
    let nonce = req.state().lock().await.new_next_nonce(from);
    let balance = req.state().lock().await.user_balance(from);
    let proof = req.state().lock().await.user_proof(from);

    let tx = Transaction::<E>::new_deposit(from, amount, fpk, nonce, balance, proof, &sk);

    if let Some(block) = req.state().lock().await.build_block(vec![tx]) {
        let rollup_hash: &String = &req.state().lock().await.rollup_lock;
        let rollup_dep_hash: &String = &req.state().lock().await.rollup_dep;
        let success_hash: &String = &req.state().lock().await.udt_lock;
        let my_udt_hash: &String = &req.state().lock().await.my_udt;
        let pre_commit_hash: &String = &req.state().lock().await.commit_cell;
        let pre_upk_hash: &String = &req.state().lock().await.upk_cell;
        let pre_udt_hash: &String = &req.state().lock().await.udt_cell;
        let block: Vec<u8> = block.to_bytes();
        let udt_amount: u128 = req.state().lock().await.total_udt_amount + amount;
        let my_udt_amount: u128 = req.state().lock().await.my_udt_amount - amount;

        let mut upks = vec![];
        for upk in &req.state().lock().await.params.proving_key.update_keys {
            let mut tmp_bytes = vec![];
            upk.write(&mut tmp_bytes).unwrap();
            upks.push(tmp_bytes);
        }

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, new_my_udt, tx_id)) = send_deposit(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            my_udt_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            upks,
            udt_amount,
            my_udt_amount,
        )
        .await
        {
            req.state().lock().await.commit_cell = new_commit_cell;
            req.state().lock().await.upk_cell = new_upk_cell;
            req.state().lock().await.udt_cell = new_udt_cell;
            req.state().lock().await.my_udt = new_my_udt;
            req.state().lock().await.my_udt_amount -= amount;

            Ok(tx_id)
        } else {
            Ok("Send Tx Failure".to_owned())
        }
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct WithdrawRequest {
    pub from: u32,
    pub amount: u128,
    pub psk: String,
}

/// wallet withddraw api. build tx and send to ckb.
async fn withdraw<E: PairingEngine>(
    mut req: Request<Arc<Mutex<Storage<E>>>>,
) -> Result<String, Error> {
    let params: WithdrawRequest = req.body_json().await?;
    let (from, amount, sk) = (
        params.from,
        params.amount,
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    if !req.state().lock().await.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let balance = req.state().lock().await.user_balance(from);

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let fpk = req.state().lock().await.user_fpk(from);
    let nonce = req.state().lock().await.new_next_nonce(from);
    let proof = req.state().lock().await.user_proof(from);

    let tx = Transaction::<E>::new_withdraw(from, amount, fpk, nonce, balance, proof, &sk);

    if let Some(block) = req.state().lock().await.build_block(vec![tx]) {
        let rollup_hash: &String = &req.state().lock().await.rollup_lock;
        let rollup_dep_hash: &String = &req.state().lock().await.rollup_dep;
        let success_hash: &String = &req.state().lock().await.udt_lock;
        let pre_commit_hash: &String = &req.state().lock().await.commit_cell;
        let pre_upk_hash: &String = &req.state().lock().await.upk_cell;
        let pre_udt_hash: &String = &req.state().lock().await.udt_cell;
        let block: Vec<u8> = block.to_bytes();
        let udt_amount: u128 = req.state().lock().await.total_udt_amount - amount;
        let my_udt_amount: u128 = amount;

        let mut upks = vec![];
        for upk in &req.state().lock().await.params.proving_key.update_keys {
            let mut tmp_bytes = vec![];
            upk.write(&mut tmp_bytes).unwrap();
            upks.push(tmp_bytes);
        }

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, tx_id)) = send_withdraw(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            upks,
            udt_amount,
            my_udt_amount,
        )
        .await
        {
            req.state().lock().await.commit_cell = new_commit_cell;
            req.state().lock().await.upk_cell = new_upk_cell;
            req.state().lock().await.udt_cell = new_udt_cell;

            Ok(tx_id)
        } else {
            Ok("Send Tx Failure".to_owned())
        }
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct TransferRequest {
    pub from: u32,
    pub to: u32,
    pub amount: u128,
    pub psk: String,
}

/// wallet transfer api. build tx and send to ckb.
async fn transfer<E: PairingEngine>(
    mut req: Request<Arc<Mutex<Storage<E>>>>,
) -> Result<String, Error> {
    let params: TransferRequest = req.body_json().await?;
    let (from, to, amount, psk) = (
        params.from,
        params.to,
        params.amount,
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    println!(
        "Recv transfer tx: from {}, to {}, amount {}",
        from, to, amount
    );

    if !req.state().lock().await.contains_users(&[from, to]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let from_fpk = req.state().lock().await.user_fpk(from);
    let nonce = req.state().lock().await.new_next_nonce(from);
    let balance = req.state().lock().await.user_balance(from);
    let proof = req.state().lock().await.user_proof(from);

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let tx =
        Transaction::<E>::new_transfer(from, to, amount, from_fpk, nonce, balance, proof, &psk);

    let tx_hash_id = tx.id();

    if req.state().lock().await.try_insert_tx(tx) {
        Ok(tx_hash_id)
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

/// wallet transfer api. build tx and send to ckb.
async fn setup<E: PairingEngine>(req: Request<Arc<Mutex<Storage<E>>>>) -> Result<String, Error> {
    //let from_fpk = req.state().lock().await.user_fpk(from);
    let (rollup_lock, rollup_dep, udt_lock, my_udt) = deploy_contract("asvc_rollup").await.unwrap();

    println!("ASVC rollup lock: {}", rollup_lock);
    println!("ASVC rollup lock dep: {}", rollup_dep);
    println!("ASVC udt lock: {}", udt_lock);
    println!("ASVC my udt ouput: {}", my_udt);

    req.state().lock().await.rollup_lock = rollup_lock.clone();
    req.state().lock().await.rollup_dep = rollup_dep.clone();
    req.state().lock().await.udt_lock = udt_lock;
    req.state().lock().await.my_udt = my_udt;
    req.state().lock().await.my_udt_amount = 100000;

    let mut commit_bytes = vec![];
    req.state()
        .lock()
        .await
        .commit
        .write(&mut commit_bytes)
        .unwrap();

    let mut upks_bytes = vec![];
    for upk in &req.state().lock().await.params.proving_key.update_keys {
        let mut tmp_bytes = vec![];
        upk.write(&mut tmp_bytes).unwrap();
        upks_bytes.push(tmp_bytes);
    }

    // send init state to chain.
    if let Ok((commit_cell, upk_cell, udt_cell, tx_id)) =
        init_state(rollup_lock, rollup_dep, commit_bytes, upks_bytes).await
    {
        req.state().lock().await.commit_cell = commit_cell;
        req.state().lock().await.upk_cell = upk_cell;
        req.state().lock().await.udt_cell = udt_cell;

        Ok(tx_id)
    } else {
        Ok("Init Failure".to_owned())
    }
}

fn main() {
    let (params, commit, proofs, full_pubkeys) = match initialize_asvc::<Bn_256>(ACCOUNT_SIZE) {
        Ok(result) => result,
        Err(error) => panic!("Problem initializing asvc: {:?}", error),
    };

    // mock storage
    let storage = Storage::<Bn_256>::init(params, commit, proofs, full_pubkeys);
    let s = Arc::new(Mutex::new(storage));

    // Running Tasks.
    task::spawn(listen_contracts(s.clone()));
    task::spawn(miner(s.clone()));

    // API server
    tide::log::start();
    let mut app = tide::with_state(s);
    app.at("/").get(|_| async { Ok("Asvc Rollup is running!") });

    // wallet service
    app.at("/deposit").post(deposit);
    app.at("/withdraw").post(withdraw);
    app.at("/transfer").post(transfer);

    // L2 service
    app.at("/setup").post(setup);
    app.at("/register").post(register);

    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
