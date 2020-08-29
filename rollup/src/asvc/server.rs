use async_std::{
    sync::{Arc, RwLock},
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
    storage: Arc<RwLock<Storage<E>>>,
) -> Result<(), std::io::Error> {
    let mut l1_block_height = 0;

    loop {
        // 10s to read lastest block to check if block has deposit tx.
        task::sleep(Duration::from_secs(1000)).await;
        println!(
            "Listen Task: start read block's txs. Current block height: {}",
            l1_block_height
        );

        let rollup_lock = &storage.read().await.rollup_lock;

        if let Ok((blocks, new_height)) = listen_blocks(l1_block_height, rollup_lock).await {
            for (bytes, new_commit, new_upk, is_new_udt) in blocks {
                if let Ok(block) = Block::from_bytes(&bytes[..]) {
                    storage.write().await.handle_block(block);

                    storage.write().await.commit_cell = new_commit;
                    storage.write().await.upk_cell = new_upk;
                    if let Some((new_udt, amount)) = is_new_udt {
                        storage.write().await.udt_cell = new_udt;
                        storage.write().await.total_udt_amount = amount;
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
async fn miner<E: PairingEngine>(storage: Arc<RwLock<Storage<E>>>) -> Result<(), std::io::Error> {
    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        let mut write_storage = storage.write().await;

        let vk = write_storage.params.verification_key.clone();
        let upks = write_storage.params.proving_key.update_keys.clone();
        let omega = write_storage.omega.clone();

        if let Some(block) = write_storage.create_block() {
            println!("SUCCESS MINER A BLOCK");

            let verify_res = block.verify(&vk, omega, &upks);
            println!("Block verify is: {:?}", verify_res);

            let rollup_hash: &String = &write_storage.rollup_lock;
            let rollup_dep_hash: &String = &write_storage.rollup_dep;
            let pre_commit_hash: &String = &write_storage.commit_cell;
            let pre_upk_hash: &String = &write_storage.upk_cell;
            let block_bytes: Vec<u8> = block.to_bytes();

            let mut omega = vec![];
            write_storage.omega.write(&mut omega).unwrap();

            let mut upks = vec![];
            for upk in &write_storage.params.proving_key.update_keys {
                let mut tmp_bytes = vec![];
                upk.write(&mut tmp_bytes).unwrap();
                upks.push(tmp_bytes);
            }

            let mut vk_bytes = Vec::new();
            write_storage
                .params
                .verification_key
                .write(&mut vk_bytes)
                .unwrap();

            if let Ok((new_commit_cell, new_upk_cell, tx_id)) = send_block(
                rollup_hash,
                rollup_dep_hash,
                pre_commit_hash,
                pre_upk_hash,
                block_bytes,
                vk_bytes,
                omega,
                upks,
            )
            .await
            {
                write_storage.commit_cell = new_commit_cell;
                write_storage.upk_cell = new_upk_cell;

                println!("block send L1 is success: tx: {}", tx_id);
                write_storage.handle_block(block);
            } else {
                write_storage.revert_block(block);
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
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: RegisterRequest = req.body_json().await?;
    println!("new next user: {:?}", params);

    let (pubkey, psk) = (
        PublicKey::from_hex(&params.pubkey).unwrap(),
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    let (account, upk) = read_storage.new_next_user();
    println!("new next user: {}", account);
    let proof = read_storage.user_proof(account).clone();

    let fpk = FullPubKey::<E> {
        i: account,
        update_key: upk,
        tradition_pubkey: pubkey.clone(),
    };

    let tx = Transaction::<E>::new_register(account, fpk, 0, 0, proof, &psk);

    let tx_id = tx.id();

    if req.state().write().await.try_insert_tx(tx) {
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
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: DepositRequest = req.body_json().await?;
    let (from, amount, sk) = (
        params.to,
        params.amount,
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    if !read_storage.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let fpk = read_storage.user_fpk(from);
    let nonce = read_storage.new_next_nonce(from);
    let balance = read_storage.user_balance(from);
    let proof = read_storage.user_proof(from);

    let tx = Transaction::<E>::new_deposit(from, amount, fpk, nonce, balance, proof, &sk);

    let mut write_storage = req.state().write().await;

    if let Some(block) = write_storage.build_block(vec![tx]) {
        let rollup_hash: &String = &write_storage.rollup_lock;
        let rollup_dep_hash: &String = &write_storage.rollup_dep;
        let success_hash: &String = &write_storage.udt_lock;
        let my_udt_hash: &String = &write_storage.my_udt;
        let pre_commit_hash: &String = &write_storage.commit_cell;
        let pre_upk_hash: &String = &write_storage.upk_cell;
        let pre_udt_hash: &String = &write_storage.udt_cell;
        let block: Vec<u8> = block.to_bytes();
        let udt_amount: u128 = write_storage.total_udt_amount + amount;
        let my_udt_amount: u128 = write_storage.my_udt_amount - amount;

        let mut omega = vec![];
        write_storage.omega.write(&mut omega).unwrap();

        let mut upks = vec![];
        for upk in &write_storage.params.proving_key.update_keys {
            let mut tmp_bytes = vec![];
            upk.write(&mut tmp_bytes).unwrap();
            upks.push(tmp_bytes);
        }

        let mut vk_bytes = Vec::new();
        req.state()
            .read()
            .await
            .params
            .verification_key
            .write(&mut vk_bytes)
            .unwrap();

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, new_my_udt, tx_id)) = send_deposit(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            my_udt_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            vk_bytes,
            omega,
            upks,
            udt_amount,
            my_udt_amount,
        )
        .await
        {
            write_storage.commit_cell = new_commit_cell;
            write_storage.upk_cell = new_upk_cell;
            write_storage.udt_cell = new_udt_cell;
            write_storage.my_udt = new_my_udt;
            write_storage.my_udt_amount -= amount;

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

/// wallet withdraw api. build tx and send to ckb.
async fn withdraw<E: PairingEngine>(
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: WithdrawRequest = req.body_json().await?;
    let (from, amount, sk) = (
        params.from,
        params.amount,
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    if !read_storage.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let balance = read_storage.user_balance(from);

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let fpk = read_storage.user_fpk(from);
    let nonce = read_storage.new_next_nonce(from);
    let proof = read_storage.user_proof(from);

    let tx = Transaction::<E>::new_withdraw(from, amount, fpk, nonce, balance, proof, &sk);

    let mut write_storage = req.state().write().await;

    if let Some(block) = write_storage.build_block(vec![tx]) {
        let rollup_hash: &String = &write_storage.rollup_lock;
        let rollup_dep_hash: &String = &write_storage.rollup_dep;
        let success_hash: &String = &write_storage.udt_lock;
        let pre_commit_hash: &String = &write_storage.commit_cell;
        let pre_upk_hash: &String = &write_storage.upk_cell;
        let pre_udt_hash: &String = &write_storage.udt_cell;
        let block: Vec<u8> = block.to_bytes();
        let udt_amount: u128 = write_storage.total_udt_amount - amount;
        let my_udt_amount: u128 = amount;

        let mut omega = vec![];
        write_storage.omega.write(&mut omega).unwrap();

        let mut upks = vec![];
        for upk in &write_storage.params.proving_key.update_keys {
            let mut tmp_bytes = vec![];
            upk.write(&mut tmp_bytes).unwrap();
            upks.push(tmp_bytes);
        }

        let mut vk_bytes = Vec::new();
        write_storage
            .params
            .verification_key
            .write(&mut vk_bytes)
            .unwrap();

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, tx_id)) = send_withdraw(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            vk_bytes,
            omega,
            upks,
            udt_amount,
            my_udt_amount,
        )
        .await
        {
            write_storage.commit_cell = new_commit_cell;
            write_storage.upk_cell = new_upk_cell;
            write_storage.udt_cell = new_udt_cell;

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
    mut req: Request<Arc<RwLock<Storage<E>>>>,
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

    if !req.state().read().await.contains_users(&[from, to]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let read_storage = req.state().read().await;

    let from_fpk = read_storage.user_fpk(from);
    let nonce = read_storage.new_next_nonce(from);
    let balance = read_storage.user_balance(from);
    let proof = read_storage.user_proof(from);

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let tx =
        Transaction::<E>::new_transfer(from, to, amount, from_fpk, nonce, balance, proof, &psk);

    let tx_hash_id = tx.id();

    if req.state().write().await.try_insert_tx(tx) {
        Ok(tx_hash_id)
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

/// wallet transfer api. build tx and send to ckb.
async fn setup<E: PairingEngine>(req: Request<Arc<RwLock<Storage<E>>>>) -> Result<String, Error> {
    //let from_fpk = req.state().read().await.user_fpk(from);
    let (rollup_lock, rollup_dep, udt_lock, my_udt) = deploy_contract("asvc_rollup").await.unwrap();

    println!("ASVC rollup lock: {}", rollup_lock);
    println!("ASVC rollup lock dep: {}", rollup_dep);
    println!("ASVC udt lock: {}", udt_lock);
    println!("ASVC my udt ouput: {}", my_udt);

    let mut storage = req.state().write().await;

    storage.rollup_lock = rollup_lock.clone();
    storage.rollup_dep = rollup_dep.clone();
    storage.udt_lock = udt_lock;
    storage.my_udt = my_udt;
    storage.my_udt_amount = 100000;

    let mut commit_bytes = vec![];
    storage.commit.write(&mut commit_bytes).unwrap();

    let mut upks_bytes = vec![];
    for upk in &storage.params.proving_key.update_keys {
        let mut tmp_bytes = vec![];
        upk.write(&mut tmp_bytes).unwrap();
        upks_bytes.push(tmp_bytes);
    }

    let mut omega = vec![];
    storage.omega.write(&mut omega).unwrap();

    let mut vk_bytes = Vec::new();
    storage
        .params
        .verification_key
        .write(&mut vk_bytes)
        .unwrap();

    // send init state to chain.
    if let Ok((commit_cell, upk_cell, udt_cell, tx_id)) = init_state(
        rollup_lock,
        rollup_dep,
        commit_bytes,
        vk_bytes,
        omega,
        upks_bytes,
    )
    .await
    {
        storage.commit_cell = commit_cell;
        storage.upk_cell = upk_cell;
        storage.udt_cell = udt_cell;

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
    let s = Arc::new(RwLock::new(storage));

    // Running Tasks.
    task::spawn(listen_contracts(s.clone()));
    task::spawn(miner(s.clone()));

    // API server
    //tide::log::start();
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
