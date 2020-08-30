use async_std::{
    sync::{Arc, RwLock},
    task,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tide::{Error, Request, StatusCode};

use ckb_zkp::curve::bn_256::Bn_256;
use ckb_zkp::math::PairingEngine;

mod asvc;
mod storage;

use asvc::initialize_asvc;
use storage::Storage;

use asvc_rollup::block::Block;
use asvc_rollup::transaction::{PublicKey, SecretKey, ACCOUNT_SIZE};
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
        task::sleep(Duration::from_secs(10)).await;
        println!(
            "Listen Task: start read block's txs. Current block height: {}",
            l1_block_height
        );

        let read_storage = storage.read().await;
        let rollup_lock = &read_storage.rollup_lock;

        if let Ok(blocks) = listen_blocks(l1_block_height, rollup_lock).await {
            drop(read_storage);
            for (block, new_height) in blocks.iter() {
                for (bytes, new_commit, new_upk, is_new_udt) in block {
                    if let Ok(block) = Block::from_bytes(&bytes[..]) {
                        let mut write_storage = storage.write().await;

                        block
                            .verify(&write_storage.cell_upks)
                            .expect("BLOCK VERIFY ERROR");

                        write_storage.handle_block(block);

                        write_storage.commit_cell = new_commit.clone();
                        write_storage.upk_cell = new_upk.clone();
                        if let Some((new_udt, amount)) = is_new_udt {
                            write_storage.udt_cell = new_udt.clone();
                            write_storage.total_udt_amount = *amount;
                        }
                        drop(write_storage);
                    }
                }

                l1_block_height = *new_height;
            }
        }

        println!(
            "Listen Task: end read block's txs. Current block height: {}",
            l1_block_height
        );
    }
}

/// Miner task.
async fn miner<E: PairingEngine>(storage: Arc<RwLock<Storage<E>>>) -> Result<(), std::io::Error> {
    let read_storage = storage.write().await;
    let cell_upks = read_storage.cell_upks.clone();
    drop(read_storage);

    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        let mut write_storage = storage.write().await;

        if let Some(block) = write_storage.create_block() {
            println!("SUCCESS MINER A BLOCK tx is: {}", block.txs.len());

            let verify_res = block.verify(&cell_upks);
            println!("Block verify is: {:?}", verify_res);

            let rollup_hash: &String = &write_storage.rollup_lock;
            let rollup_dep_hash: &String = &write_storage.rollup_dep;
            let pre_commit_hash: &String = &write_storage.commit_cell;
            let pre_upk_hash: &String = &write_storage.upk_cell;
            let block_bytes: Vec<u8> = block.to_bytes();

            if let Ok((new_commit_cell, new_upk_cell, tx_id)) = send_block(
                rollup_hash,
                rollup_dep_hash,
                pre_commit_hash,
                pre_upk_hash,
                block_bytes,
                cell_upks.to_bytes(),
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

        drop(write_storage);
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

    let (pubkey, sk) = (
        PublicKey::from_hex(&params.pubkey).unwrap(),
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    let account = read_storage.next_user();
    let tx = read_storage.new_register(account, pubkey, &sk);

    drop(read_storage);

    let tx_id = tx.id();

    let mut write_storage = req.state().write().await;

    if write_storage.try_insert_tx(tx) {
        drop(write_storage);
        Ok(tx_id)
    } else {
        drop(write_storage);
        Ok("Invalid Tx".to_owned())
    }
}

/// wallet deposit api. build tx and send to ckb.
async fn deposit<E: PairingEngine>(
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: Value = req.body_json().await?;

    let from: u32 = params["to"].as_str().unwrap().parse().unwrap();
    let amount: u128 = params["amount"].as_str().unwrap().parse().unwrap();
    let sk = SecretKey::from_hex(params["psk"].as_str().unwrap()).unwrap();
    println!("[deposit] from={}, amount={}", from, amount);

    let read_storage = req.state().read().await;

    if !read_storage.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let tx = read_storage.new_deposit(from, amount, &sk);

    drop(read_storage);

    let mut write_storage = req.state().write().await;

    if let Some(block) = write_storage.build_block_by_user(tx) {
        let res = block.verify(&write_storage.cell_upks);
        //.expect("DEPOSIT BLOCK VERIFY FAILURE");

        println!("=========BLOCK DEPOSIT: {:?}", res);

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

        let cell_upks = write_storage.cell_upks.to_bytes();

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, new_my_udt, tx_id)) = send_deposit(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            my_udt_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            cell_upks,
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

            drop(write_storage);
            Ok(tx_id)
        } else {
            drop(write_storage);
            Ok("Send Tx Failure".to_owned())
        }
    } else {
        drop(write_storage);
        Ok("Invalid Tx".to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct WithdrawRequest {
    pub from: String,
    pub amount: String,
    pub psk: String,
}

/// wallet withdraw api. build tx and send to ckb.
async fn withdraw<E: PairingEngine>(
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: WithdrawRequest = req.body_json().await?;
    let (from, amount, sk) = (
        params.from.parse().unwrap(),
        params.amount.parse().unwrap(),
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    if !read_storage.contains_users(&[from]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let balance = read_storage.pool_balance(from);

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let tx = read_storage.new_withdraw(from, amount, &sk);
    drop(read_storage);

    let mut write_storage = req.state().write().await;

    if let Some(block) = write_storage.build_block_by_user(tx) {
        block
            .verify(&write_storage.cell_upks)
            .expect("WITHDRAW BLOCK VERIFY FAILURE");

        let rollup_hash: &String = &write_storage.rollup_lock;
        let rollup_dep_hash: &String = &write_storage.rollup_dep;
        let success_hash: &String = &write_storage.udt_lock;
        let pre_commit_hash: &String = &write_storage.commit_cell;
        let pre_upk_hash: &String = &write_storage.upk_cell;
        let pre_udt_hash: &String = &write_storage.udt_cell;
        let block: Vec<u8> = block.to_bytes();
        let udt_amount: u128 = write_storage.total_udt_amount - amount;
        let my_udt_amount: u128 = amount;

        let cell_upks = write_storage.cell_upks.to_bytes();

        if let Ok((new_commit_cell, new_upk_cell, new_udt_cell, tx_id)) = send_withdraw(
            rollup_hash,
            rollup_dep_hash,
            success_hash,
            pre_commit_hash,
            pre_upk_hash,
            pre_udt_hash,
            block,
            cell_upks,
            udt_amount,
            my_udt_amount,
        )
        .await
        {
            write_storage.commit_cell = new_commit_cell;
            write_storage.upk_cell = new_upk_cell;
            write_storage.udt_cell = new_udt_cell;

            drop(write_storage);
            Ok(tx_id)
        } else {
            drop(write_storage);
            Ok("Send Tx Failure".to_owned())
        }
    } else {
        drop(write_storage);
        Ok("Invalid Tx".to_owned())
    }
}

#[derive(Serialize, Deserialize)]
struct TransferRequest {
    pub from: String,
    pub to: String,
    pub amount: String,
    pub psk: String,
}

/// wallet transfer api. build tx and send to ckb.
async fn transfer<E: PairingEngine>(
    mut req: Request<Arc<RwLock<Storage<E>>>>,
) -> Result<String, Error> {
    let params: TransferRequest = req.body_json().await?;
    let (from, to, amount, sk) = (
        params.from.parse().unwrap(),
        params.to.parse().unwrap(),
        params.amount.parse().unwrap(),
        SecretKey::from_hex(&params.psk).unwrap(),
    );

    let read_storage = req.state().read().await;

    if !read_storage.contains_users(&[from, to]) {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user number is invalid",
        ));
    }

    let balance = read_storage.pool_balance(from);

    println!(
        "transfer balance: from balance: {}, from: {}, to: {}, amount {}",
        balance, from, to, amount
    );

    if amount > balance {
        return Err(Error::from_str(
            StatusCode::BadRequest,
            "the user balance not enough",
        ));
    }

    let tx = read_storage.new_transfer(from, to, amount, &sk);

    let tx_hash_id = tx.id();
    drop(read_storage);

    let mut write_storage = req.state().write().await;
    if write_storage.try_insert_tx(tx) {
        drop(write_storage);
        Ok(tx_hash_id)
    } else {
        drop(write_storage);
        Ok("Invalid Tx".to_owned())
    }
}

/// wallet transfer api. build tx and send to ckb.
async fn setup<E: PairingEngine>(req: Request<Arc<RwLock<Storage<E>>>>) -> Result<String, Error> {
    //let from_fpk = req.state().read().await.user_fpk(from);
    let (rollup_lock, rollup_dep, udt_lock, my_udt) =
        deploy_contract("asvc_verifier").await.unwrap();

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

    let block = Block {
        block_height: 0,
        commit: storage.commit.clone(),
        proof: storage.proofs[0].clone(),
        new_commit: storage.commit.clone(),
        txs: vec![],
    };

    let cell_upks = storage.cell_upks.to_bytes();

    // send init state to chain.
    if let Ok((commit_cell, upk_cell, udt_cell, tx_id)) =
        init_state(rollup_lock, rollup_dep, block.to_bytes(), cell_upks).await
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
