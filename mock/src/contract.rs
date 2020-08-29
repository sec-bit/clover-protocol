use async_std::{
    sync::{Arc, RwLock},
    task,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tide::{Body, Error, Request, Response};

use ckb_testtool::{builtin::ALWAYS_SUCCESS, context::Context};
use ckb_tool::ckb_types::{bytes::Bytes, core::Capacity, packed::*, prelude::*};

const MAX_CYCLES: u64 = 10_000_000;

fn jsonrpc(params: Value) -> Value {
    json!(
        {
            "id": 0,
            "jsonrpc": "2.0",
            "result": params,
        }
    )
}

/// Miner task.
async fn miner(blockchain: Arc<RwLock<Blockchain>>) -> Result<(), std::io::Error> {
    loop {
        // 10s to miner a block. (mock consensus)
        task::sleep(Duration::from_secs(10)).await;

        blockchain.write().await.miner_block();
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct DeployReq {
    pub contract: String,
}

async fn deploy(mut req: Request<Arc<RwLock<Blockchain>>>) -> Result<Response, Error> {
    println!("start deploy contract.........");
    let rpc: DeployReq = req.body_json().await?;
    let rollup_bin: Bytes = std::fs::read(format!("./build/debug/{}", rpc.contract))
        .expect("binary")
        .into();

    let mut blockchain = req.state().write().await;

    let success_point = blockchain.context.deploy_cell(ALWAYS_SUCCESS.clone());
    let success_lock_script = blockchain
        .context
        .build_script(&success_point, Default::default())
        .expect("script");

    let rollup_point = blockchain.context.deploy_cell(rollup_bin);
    let rollup_script_args: Bytes = [0u8; 1].to_vec().into();
    let rollup_lock_script = blockchain
        .context
        .build_script(&rollup_point, rollup_script_args)
        .expect("script");
    let rollup_lock_script_dep = CellDep::new_builder().out_point(rollup_point).build();

    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();
    // init UDT 100,000
    let my_udt_point = blockchain.context.create_cell(
        CellOutput::new_builder()
            .capacity(input_ckb.pack())
            .lock(success_lock_script.clone())
            .build(),
        100000u128.to_le_bytes().to_vec().into(),
    );

    let mut res = Response::new(200);
    res.set_body(Body::from_json(&jsonrpc(json!(vec![
        hex::encode(rollup_lock_script.as_slice()),
        hex::encode(rollup_lock_script_dep.as_slice()),
        hex::encode(success_lock_script.as_slice()),
        hex::encode(my_udt_point.as_slice())
    ])))?);

    Ok(res)
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct RpcReq {
    pub id: u32,
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<String>,
}

async fn rpc(mut req: Request<Arc<RwLock<Blockchain>>>) -> Result<Response, Error> {
    let rpc: RpcReq = req.body_json().await?;
    let (method, params) = (rpc.method.as_str(), rpc.params);

    let results = match method {
        "get_tip_block_number" => json!(req.state().read().await.get_block_height()),
        "get_block" => {
            let txs: Vec<String> = req
                .state()
                .read()
                .await
                .get_block(params[0].parse().unwrap())
                .iter()
                .map(|tx| hex::encode(tx.as_slice()))
                .collect();

            json!(txs)
        }
        "send_transaction" => {
            let tx_bytes = hex::decode(&params[0]).unwrap();
            let tx = TransactionView::new_unchecked(tx_bytes.into());

            let mut blockchain = req.state().write().await;

            let cycles = blockchain
                .context
                .verify_tx(&tx.unpack(), MAX_CYCLES)
                .expect("pass verification");
            println!("Tx cycles: {}...", cycles);

            let tx_hash = tx.hash();
            blockchain.pool.insert(tx_hash.clone(), tx);

            json!(hex::encode(tx_hash.as_slice()))
        }
        _ => json!("Not supported"),
    };

    let mut res = Response::new(200);
    res.set_body(Body::from_json(&jsonrpc(results))?);
    Ok(res)
}

struct IndexBlock {
    pub txs: Vec<Byte32>,
}

struct Blockchain {
    context: Context,
    pool: HashMap<Byte32, TransactionView>,
    stable_txs: HashMap<Byte32, TransactionView>,
    blocks: HashMap<u32, IndexBlock>,
}

impl Blockchain {
    fn miner_block(&mut self) {
        let mut block = vec![];
        for (tx_hash, tx) in self.pool.drain() {
            self.context.complete_tx(tx.unpack());

            block.push(tx_hash.clone());
            self.stable_txs.insert(tx_hash, tx);
        }

        let current_height = self.blocks.len() as u32;
        println!("Miner new block: {}, txs: {}", current_height, block.len());
        self.blocks
            .insert(current_height, IndexBlock { txs: block });
    }

    fn get_block_height(&self) -> u32 {
        self.blocks.len() as u32
    }

    fn get_block(&self, height: u32) -> Vec<TransactionView> {
        if let Some(block) = self.blocks.get(&height) {
            let mut txs = vec![];
            for tx in &block.txs {
                txs.push(self.stable_txs.get(tx).unwrap().clone());
            }
            txs
        } else {
            vec![]
        }
    }
}

impl Default for Blockchain {
    fn default() -> Self {
        Self {
            context: Context::default(),
            pool: HashMap::new(),
            stable_txs: HashMap::new(),
            blocks: HashMap::new(),
        }
    }
}

fn main() {
    tide::log::start();

    let blockchain = Arc::new(RwLock::new(Blockchain::default()));

    task::spawn(miner(blockchain.clone()));

    let mut app = tide::with_state(blockchain);

    // contracts
    app.at("/deploy").post(deploy);

    // node
    app.at("/").post(rpc);

    task::block_on(app.listen("127.0.0.1:8114")).unwrap();
}
