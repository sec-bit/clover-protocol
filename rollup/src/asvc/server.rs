use async_std::{
    sync::{Arc, Mutex},
    task,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tide::{Error, Request, StatusCode};

use ckb_zkp::curve::bn_256::Bn_256;
use ckb_zkp::math::PairingEngine;

mod asvc;
mod storage;

use asvc::initialize_asvc;
use storage::Storage;

use asvc_rollup::block::Block;
use asvc_rollup::transaction::{FullPubKey, PublicKey, SecretKey, Transaction, ACCOUNT_SIZE};
use ckb_rpc::{listen_blocks, send_block, send_deposit, send_withdraw};

/// listening task.
async fn listen_contracts<E: PairingEngine>(
    s: Arc<Mutex<Storage<E>>>,
) -> Result<(), std::io::Error> {
    let mut l1_block_height = 0;

    loop {
        // 10s to read lastest block to check if block has deposit tx.
        task::sleep(Duration::from_secs(10)).await;
        println!(
            "Listen Task: start read block's txs. Current block height: {}",
            l1_block_height
        );

        if let Ok((blocks, new_height)) = listen_blocks(l1_block_height).await {
            for bytes in blocks {
                if let Ok(block) = Block::from_bytes(&bytes[..]) {
                    s.lock().await.handle_block(block);
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

            if let Ok(res) = send_block(block.to_bytes()).await {
                println!("block send L1 is success: tx: {}", res);
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

    let tx_hash_id = tx.id();

    if let Some(block) = req.state().lock().await.build_block(vec![tx]) {
        if let Ok(res) = send_deposit(block.to_bytes()).await {
            println!("Send CKB Tx: {}", res);
            return Ok(tx_hash_id);
        }

        Ok("Send Tx Failure".to_owned())
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

    let tx_hash_id = tx.id();

    if let Some(block) = req.state().lock().await.build_block(vec![tx]) {
        if let Ok(res) = send_withdraw(block.to_bytes()).await {
            println!("Send CKB Tx: {}", res);
            return Ok(tx_hash_id);
        }

        Ok("Send Tx Failure".to_owned())
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

fn main() {
    let (params, commit, proofs, full_pubkeys) = match initialize_asvc::<Bn_256>(ACCOUNT_SIZE) {
        Ok(result) => result,
        Err(error) => panic!("Problem initializing asvc: {:?}", error),
    };

    // TODO: submit to contract

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
    //app.at("/send_tx").post(send_tx);
    app.at("/register").post(register);

    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
