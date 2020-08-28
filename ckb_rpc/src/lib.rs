use serde_json::{json, Value};

use ckb_tool::ckb_types::{
    bytes::Bytes,
    core::{Capacity, TransactionBuilder},
    packed::*,
    prelude::*,
};

const NODE_RPC_ADDR: &'static str = "http://127.0.0.1:8114";

fn jsonrpc(method: &str, params: Value) -> Value {
    json!(
        {
            "id": 0,
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }
    )
}

pub async fn deploy_contract(name: &str) -> Result<(String, String, String), ()> {
    match surf::post(format!("{}/deploy", NODE_RPC_ADDR))
        .body_json(&json!({ "contract": name }))
        .map_err(|_e| ())?
        .await
    {
        Ok(mut res) => match res.body_json::<Value>().await {
            Ok(mut value) => {
                let result = value["result"].take();
                let rollup_lock = result[0].as_str().ok_or(())?;
                let rollup_lock_dep = result[1].as_str().ok_or(())?;
                let udt_lock = result[2].as_str().ok_or(())?;

                Ok((
                    rollup_lock.to_owned(),
                    rollup_lock_dep.to_owned(),
                    udt_lock.to_owned(),
                ))
            }
            Err(err) => {
                println!("JSONRPC err: {:?}", err);
                Err(())
            }
        },
        Err(err) => {
            println!("RPC deploy contract error: {:?}", err);
            Err(())
        }
    }
}

pub async fn listen_blocks(block_height: u64) -> Result<(Vec<Vec<u8>>, u64), ()> {
    //get_tip_block_number
    let now_height = match surf::post(NODE_RPC_ADDR)
        .body_json(&jsonrpc("get_tip_block_number", Default::default()))
        .map_err(|_e| ())?
        .await
    {
        Ok(mut res) => match res.body_json::<Value>().await {
            Ok(mut value) => {
                let result = value["result"].take();
                result.as_u64().ok_or(())?
            }
            Err(err) => {
                println!("JSONRPC err: {:?}", err);
                return Err(());
            }
        },
        Err(err) => {
            println!("Listening query err: {:?}", err);
            return Err(());
        }
    };

    println!("now_height: {:?}", now_height);

    if now_height <= block_height {
        return Ok((vec![], block_height));
    }

    let mut blocks = vec![];
    let mut change_block_height = block_height;

    for i in block_height..now_height {
        // get block info
        if let Ok(mut res) = surf::post(NODE_RPC_ADDR)
            .body_json(&jsonrpc("get_block", json!(vec![i])))
            .map_err(|_e| ())?
            .await
        {
            let result = res.body_json::<Value>().await.map_err(|_| ())?;
            let transactions = result["result"]["transactions"].as_array().ok_or(())?;

            for tx in transactions {
                println!("{:?}", tx);
                // TODO CHECK Tx is to our contract.
            }
        } else {
            break;
        }
    }

    Ok((blocks, block_height))
}

pub async fn _listen_true_blocks(block_height: u64) -> Result<(Vec<Vec<u8>>, u64), ()> {
    //get_tip_block_number
    let now_height = match surf::post(NODE_RPC_ADDR)
        .body_json(&jsonrpc("get_tip_block_number", Default::default()))
        .map_err(|_e| ())?
        .await
    {
        Ok(mut res) => match res.body_json::<Value>().await {
            Ok(mut value) => {
                let result = value["result"].take();
                let hex_num = result.as_str().ok_or(())?;
                u64::from_str_radix(&hex_num[2..], 16).map_err(|_| ())?
            }
            Err(err) => {
                println!("Listening err: {:?}", err);
                return Err(());
            }
        },
        Err(err) => {
            println!("Listening query err: {:?}", err);
            return Err(());
        }
    };

    println!("now_height: {:?}", now_height);

    if now_height <= block_height {
        return Ok((vec![], block_height));
    }

    let blocks = vec![];
    let _change_block_height = block_height;

    for i in block_height..now_height {
        // get block hash
        if let Ok(mut res) = surf::post(NODE_RPC_ADDR)
            .body_json(&jsonrpc(
                "get_header_by_number",
                json!(vec![&format!("{:#x}", i)]),
            ))
            .map_err(|_e| ())?
            .await
        {
            let result = res.body_json::<Value>().await.map_err(|_| ())?;
            let hash = result["result"]["hash"].as_str().ok_or(())?;

            // get block info
            if let Ok(mut res) = surf::post(NODE_RPC_ADDR)
                .body_json(&jsonrpc("get_block", json!(vec![hash])))
                .map_err(|_e| ())?
                .await
            {
                let result = res.body_json::<Value>().await.map_err(|_| ())?;
                let transactions = result["result"]["transactions"].as_array().ok_or(())?;

                for tx in transactions {
                    //println!("{:?}", tx);
                    // TODO CHECK Tx is to our contract.
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok((blocks, block_height))
}

/// init state of L2
pub async fn init_state(
    rollup_hash: String,
    rollup_dep_hash: String,
    mut commit: Vec<u8>,
    upks: Vec<Vec<u8>>,
) -> Result<(String, String, String, String), ()> {
    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();
    let rollup_lock = Script::new_unchecked(hex::decode(rollup_hash).unwrap().into());
    let rollup_dep = CellDep::new_unchecked(hex::decode(rollup_dep_hash).unwrap().into());

    println!("start init state...");
    let init_output_commit = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();
    let init_upk = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();
    let init_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();

    commit.extend_from_slice(&mut 0u32.to_le_bytes()[..]);

    let mut all_upks = vec![];
    all_upks.extend_from_slice(&mut (upks.len() as u32).to_le_bytes()[..]);
    for mut upk in upks {
        all_upks.extend_from_slice(&mut upk[..]);
    }

    let init_outputs_data: Vec<Bytes> = vec![
        commit.into(),
        all_upks.into(),
        0u128.to_le_bytes().to_vec().into(),
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![])
        .outputs(vec![
            init_output_commit.clone(),
            init_upk.clone(),
            init_udt.clone(),
        ])
        .outputs_data(init_outputs_data.pack())
        .cell_dep(rollup_dep)
        .build();

    Ok((
        hex::encode(init_output_commit.as_slice()),
        hex::encode(init_upk.as_slice()),
        hex::encode(init_udt.as_slice()),
        send_tx(tx.pack()).await?,
    ))
}

pub async fn post_block(_block: Vec<u8>, prev: String) -> Result<String, ()> {
    let prev = "TODO";
    let contract = "TODO";

    let prev_point = OutPoint::new_unchecked(Bytes::from(prev));
    let lock_point = OutPoint::new_unchecked(Bytes::from(contract.clone()));
    let lock_script_point = Script::new_unchecked(Bytes::from(contract));
    let dep_point = CellDep::new_builder().out_point(lock_point).build();

    let input = CellInput::new_builder().previous_output(prev_point).build();

    let output = CellOutput::new_builder()
        .capacity(500u64.pack())
        .lock(lock_script_point)
        .build();

    let outputs_data = vec![Bytes::new(); 2];

    // build transaction
    let tx = TransactionBuilder::default()
        .inputs(vec![input])
        .outputs(vec![output])
        .outputs_data(outputs_data.pack())
        .cell_dep(dep_point)
        .build();

    send_tx(tx.pack()).await
}

pub async fn send_deposit(_block: Vec<u8>) -> Result<String, ()> {
    println!("TODO send deposit tx to CKB");

    Ok("TODO".to_owned())
}

pub async fn send_withdraw(_block: Vec<u8>) -> Result<String, ()> {
    println!("TODO send withdraw tx to CKB");

    Ok("TODO".to_owned())
}

pub async fn send_block(_block: Vec<u8>) -> Result<String, ()> {
    println!("TODO send block tx to CKB");

    Ok("TODO".to_owned())
}

async fn send_tx(tx: TransactionView) -> Result<String, ()> {
    let s = hex::encode(tx.as_slice());

    // Build a CKB Transaction
    let rpc_call = json!(
        {
            "id": 0,
            "jsonrpc": "2.0",
            "method": "send_transaction",
            "params": [s],
        }
    );

    // NODE RPC send_transaction
    match surf::post(NODE_RPC_ADDR)
        .body_json(&rpc_call)
        .map_err(|_e| ())?
        .await
    {
        Ok(mut res) => match res.body_json::<Value>().await {
            Ok(value) => Ok(value["result"].as_str().unwrap().to_owned()),
            Err(err) => {
                println!("{:?}", err);
                Err(())
            }
        },
        Err(err) => {
            println!("{:?}", err);
            Err(())
        }
    }
}

//
// Transaction:
// {
//     "cell_deps": [],
//     "hash": "0x365698b50ca0da75dca2c87f9e7b563811d3b5813736b8cc62cc3b106faceb17",
//     "header_deps": [],
//     "inputs": [
//         {
//             "previous_output": {
//                 "index": "0xffffffff",
//                 "tx_hash": "0x0000000000000000000000000000000000..."
//             },
//             "since": "0x400"
//         }
//     ],
//     "outputs": [
//         {
//             "capacity": "0x18e64b61cf",
//             "lock": {
//                 "args": "0x",
//                 "code_hash": "0x28e83a1277d48add8e72fadaa9248559e1b...bc4c03800a5",
//                 "hash_type": "data"
//             },
//             "type": null
//         }
//     ],
//     "outputs_data": [
//         "0x"
//     ],
//     "version": "0x0",
//     "witnesses": [
//         "0x450000000c0000004100000...c4c03800a5000000000000000000"
//     ]
// }
