use async_std::{
    sync::{Arc, Mutex},
    task,
};
use std::time::Duration;
use tide::{Error, Request};

mod storage;
use storage::Storage;

/// listening task.
async fn listen_deposit_contracts(_s: Arc<Mutex<Storage>>) -> Result<(), std::io::Error> {
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

/// listening task.
async fn miner(storage: Arc<Mutex<Storage>>) -> Result<(), std::io::Error> {
    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        if let Some(_block) = storage.lock().await.create_block() {
            // TODO Send to L1
        }
    }
}

async fn send_tx(mut req: Request<Arc<Mutex<Storage>>>) -> Result<String, Error> {
    let data = req.body_string().await?;
    println!("Recv deposit hex: {:?}, len: {}", data, data.len());

    Ok("0x".to_owned())
}

fn main() -> Result<(), std::io::Error> {
    // mock storage
    let storage = Storage::init();
    let s = Arc::new(Mutex::new(storage));

    // Running Tasks.
    task::spawn(listen_deposit_contracts(s.clone()))？;
    task::spawn(miner(s.clone()))?;

    // API server
    tide::log::start();
    let mut app = tide::with_state(s);
    app.at("/").get(|_| async { Ok("Asvc Rollup is running!") });

    app.at("/send_tx").post(send_tx);

    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
