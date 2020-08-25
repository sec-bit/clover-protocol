use async_std::{
    sync::{Arc, Mutex},
    task,
};
use std::time::Duration;
use tide::{Error, Request, StatusCode};

use ckb_zkp::math::{PairingEngine,  Zero};
use ckb_zkp::curve::bn_256::Bn_256;
use ckb_zkp::curve::PrimeField;
use core::ops::Mul;
use core::str::FromStr;
use mimc_rs::{Mimc7, generate_constants,hash};
use num_bigint::BigInt;

mod account;
mod block;
mod storage;
mod transaction;
mod asvc;

use storage::Storage;
use transaction::{Transaction, FullPubKey};
use asvc::initialize_asvc;


/// listening task.
async fn listen_contracts<E: PairingEngine>(_s: Arc<Mutex<Storage::<E>>>) -> Result<(), std::io::Error> {
    let mut l1_block_height = 0;

    loop {
        // 10s to read lastest block to check if block has deposit tx.
        task::sleep(Duration::from_secs(10)).await;
        println!(
            "Listen Task: start read block's txs. Current block height: {}",
            l1_block_height
        );

        // TODO
        l1_block_height += 1;

        println!(
            "Listen Task: end read block's txs. Current block height: {}",
            l1_block_height
        );
    }
}

/// Miner task.
async fn miner<E: PairingEngine> (storage: Arc<Mutex<Storage::<E>>>) -> Result<(), std::io::Error> {
    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        if let Some(block) = storage.lock().await.create_block() {
            if let Ok(mut res) = surf::post("http://127.0.0.1:8000/block")
                .body_string(block.to_hex())
                .await
            {
                println!(
                    "block send L1 is success: {}",
                    res.body_string().await.unwrap_or("None".to_owned())
                );
                storage.lock().await.handle_block(block);
            } else {
                storage.lock().await.revert_block(block);
            }
        }
    }
}

/// send transaction api.
async fn send_tx<E: PairingEngine>(mut req: Request<Arc<Mutex<Storage::<E>>>>) -> Result<String, Error> {
    let mut tx: Transaction<E> = req.body_json().await?;
    println!("Recv tx: {:?}", tx.hash());

    let i = tx.i;
    let j = tx.j;

    let block_height = req.state().lock().await.block_height;
    let user_height = req.state().lock().await.next_user;
    let commit = req.state().lock().await.commit;
    let n = req.state().lock().await.size;
    let full_pubkey = req.state().lock().await.full_pubkeys[i as usize];
    let j_updatekey = req.state().lock().await.full_pubkeys[j as usize].updateKey;
    let value = req.state().lock().await.values[i as usize];
    let nonce = req.state().lock().await.nonces[i as usize];
    let proof = req.state().lock().await.proofs[i as usize];

    if user_height <= i  || i < 0{
        return Err(Error::from_str(StatusCode::Ok, "the user number is invalid"))
    }

    if user_height <= j  || j < 0{
        return Err(Error::from_str(StatusCode::Ok, "the user number is invalid"))
    }

    if nonce >= tx.nonce{
        return Err(Error::from_str(StatusCode::Ok, "the user nonce is invalid"))
    }

    let balance = req.state().lock().await.balances[i as usize];
    let tx = Transaction::<E>{
        tx_type: 1 as u8,
        full_pubkey: full_pubkey,
        i: i,
        value: tx.value,
        j: j,
        j_updatekey: j_updatekey,
        nonce:tx.nonce,
        proof: proof,
        balance: balance,  //TODO: 处理注册之前存的钱
        addr: E::Fr::zero(),
    };
    
    if req.state().lock().await.try_insert_tx(tx) {
        Ok("0x".to_owned())
    } else {
        Ok("Invalid Tx".to_owned())
    }
}



pub struct RegisterRequest {
    pub pubkey: String,
}

async fn register<E: PairingEngine>(mut req: Request<Arc<Mutex<Storage::<E>>>>) -> Result<String, Error> {
    let reg: RegisterRequest = req.body_json().await?;
    // println!("Recv tx: {:?}", tx.hash());

    let user_height = req.state().lock().await.next_user;
    let commit = req.state().lock().await.commit;
    let n = req.state().lock().await.size;

    let update_keys = req.state().lock().await.params.proving_key.update_keys[user_height as usize];
    let new_full_pubkey = FullPubKey::<E>{
        i: user_height,
        updateKey: update_keys,
        traditionPubKey: reg.pubkey, 
    };
    req.state().lock().await.tmp_user_height_increment();

    let proof = req.state().lock().await.proofs[user_height as usize];
    let addr = calcuAddr(new_full_pubkey)?;
   
    let tx = Transaction::<E>{
        tx_type: 2 as u8,
        full_pubkey: new_full_pubkey,
        i: user_height,
        value: 0,
        j: 0,
        j_updatekey: update_keys,
        nonce: 0,
        proof: proof,
        balance: 0,  //TODO: 处理注册之前存的钱
        addr: addr.mul(&E::Fr::from_repr((2u64.pow(50)).into()))
    };

    if req.state().lock().await.try_insert_tx(tx) {
        Ok("0x".to_owned())
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

fn calcuAddr<E: PairingEngine>(full_pubkey: FullPubKey::<E>) ->  Result<E::Fr, Error>{
    // mimc
    let constants = generate_constants();
    let mut big_arr1: Vec<BigInt> = Vec::new();
    let bi: BigInt = BigInt::parse_bytes(b"i", full_pubkey.i).unwrap();
    big_arr1.push(bi.clone());
    let h1 = hash(big_arr1).unwrap();
    //TODO: add upk
    let result = E::Fr::from_str(h1.to_string());
    let value = match result {
        Ok(result) => result,
        Err(error) => {
           return Err(Error::from_str(StatusCode::Ok, "failed to calculate address"));
        },
    };
    Ok(value)
}

/// withdraw api.

fn main() {

    let size = 1024;
    let init_result = initialize_asvc::<Bn_256>(size);
    let (params, commit, proofs) = match init_result {
        Ok(result) => result,
        Err(error) => {
            panic!("Problem initializing asvc: {:?}", error)
        },
    };

    // TODO: submit to contract
    
    // mock storage
    let storage = Storage::<Bn_256>::init(params, commit, proofs);
    let s = Arc::new(Mutex::new(storage));

    // Running Tasks.
    task::spawn(listen_contracts(s.clone()));
    task::spawn(miner(s.clone()));

    // API server
    tide::log::start();
    let mut app = tide::with_state(s);
    app.at("/").get(|_| async { Ok("Asvc Rollup is running!") });

    app.at("/send_tx").post(send_tx);
    app.at("/register").post(register);

    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
