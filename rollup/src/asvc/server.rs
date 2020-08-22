use async_std::{
    sync::{Arc, Mutex},
    task,
};
use std::time::Duration;
use tide::{Error, Request};

mod account;
mod block;
mod storage;
mod transaction;

use storage::Storage;
use transaction::Transaction;

/// listening task.
async fn listen_contracts(_s: Arc<Mutex<Storage>>) -> Result<(), std::io::Error> {
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
async fn miner(storage: Arc<Mutex<Storage>>) -> Result<(), std::io::Error> {
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
async fn send_tx(mut req: Request<Arc<Mutex<Storage>>>) -> Result<String, Error> {
    let tx: Transaction = req.body_json().await?;
    println!("Recv tx: {:?}", tx.hash());

    if req.state().lock().await.try_insert_tx(tx) {
        Ok("0x".to_owned())
    } else {
        Ok("Invalid Tx".to_owned())
    }
}

/// withdraw api.

fn main() {
    // mock storage
    let storage = Storage::init();
    let s = Arc::new(Mutex::new(storage));

    // Running Tasks.
    task::spawn(listen_contracts(s.clone()));
    task::spawn(miner(s.clone()));

    // API server
    tide::log::start();
    let mut app = tide::with_state(s);
    app.at("/").get(|_| async { Ok("Asvc Rollup is running!") });

    app.at("/send_tx").post(send_tx);

    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
