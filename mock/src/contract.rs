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

// 1 is error
const MAX_CYCLES: u64 = 5_000_000_000;

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

        let mut write_storage = blockchain.write().await;
        write_storage.miner_block();
        drop(write_storage);
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct DeployReq {
    pub contract: String,
}

async fn deploy(mut req: Request<Arc<RwLock<Blockchain>>>) -> Result<Response, Error> {
    println!("start deploy contract.........");
    let rpc: DeployReq = req.body_json().await?;
    let rollup_bin: Bytes = std::fs::read(format!("./build/release/{}", rpc.contract))
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
            let mut txs: Vec<Vec<Vec<String>>> = vec![];
            for (_tx_hash, outptus) in req
                .state()
                .read()
                .await
                .get_block(params[0].parse().unwrap())
            {
                let mut outs = vec![];
                for out in outptus {
                    let (s, p, d) = out;
                    outs.push(vec![
                        hex::encode(s.as_slice()),
                        hex::encode(p.as_slice()),
                        hex::encode(d),
                    ]);
                }
                txs.push(outs);
            }

            json!(txs)
        }
        "send_transaction" => {
            let tx_bytes = hex::decode(&params[0]).unwrap();
            let tx = TransactionView::new_unchecked(tx_bytes.into());

            println!(
                "receive send_transaction: {}",
                hex::encode(tx.hash().as_slice())
            );

            let mut blockchain = req.state().write().await;

            let new_tx = blockchain.context.complete_tx(tx.unpack());

            println!("start verify tx...");
            let cycles = blockchain
                .context
                .verify_tx(&new_tx, MAX_CYCLES)
                .expect("pass verification");
            println!("Tx cycles: {}...", cycles);

            // MOCK: context create_ouput_cell for next call.
            let mut results = vec![];
            let mut mock_tx = vec![];

            for i in 0..new_tx.outputs().len() {
                let output = new_tx.outputs().get(i).unwrap();
                let data: Bytes = new_tx.outputs_data().get(i).unwrap().unpack();
                println!(
                    "cell {} data len: {}, pack len: {}",
                    i,
                    data.len(),
                    data.pack().len()
                );
                let lock = output.lock();
                let out_point = blockchain.context.create_cell(output, data.clone());

                results.push(hex::encode(out_point.as_slice()));
                mock_tx.push((lock, out_point, data));
            }

            let tx_hash = tx.hash();

            blockchain.pool.insert(tx_hash.clone(), mock_tx);

            json!(results)
        }
        _ => json!("Not supported"),
    };

    let mut res = Response::new(200);
    res.set_body(Body::from_json(&jsonrpc(results))?);
    Ok(res)
}

/// lock script, outpoint, data
type MockTx = Vec<(Script, OutPoint, Bytes)>;

struct Blockchain {
    context: Context,
    pool: HashMap<Byte32, MockTx>,
    blocks: HashMap<u32, HashMap<Byte32, MockTx>>,
}

impl Blockchain {
    fn miner_block(&mut self) {
        let mut block = HashMap::new();
        for (hash, data) in self.pool.drain() {
            block.insert(hash, data);
        }

        let current_height = self.blocks.len() as u32;
        println!("Miner new block: {}, txs: {}", current_height, block.len());
        self.blocks.insert(current_height, block);
    }

    fn get_block_height(&self) -> u32 {
        self.blocks.len() as u32
    }

    fn get_block(&self, height: u32) -> HashMap<Byte32, MockTx> {
        if let Some(block) = self.blocks.get(&height) {
            block.clone()
        } else {
            HashMap::new()
        }
    }
}

impl Default for Blockchain {
    fn default() -> Self {
        Self {
            context: Context::default(),
            pool: HashMap::new(),
            blocks: HashMap::new(),
        }
    }
}

fn main() {
    //tide::log::start();

    let blockchain = Arc::new(RwLock::new(Blockchain::default()));

    task::spawn(miner(blockchain.clone()));

    let mut app = tide::with_state(blockchain);

    // contracts
    app.at("/deploy").post(deploy);

    // node
    app.at("/").post(rpc);

    task::block_on(app.listen("127.0.0.1:8114")).unwrap();
}
