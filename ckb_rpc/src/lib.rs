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

pub async fn deploy_contract(name: &str) -> Result<(String, String, String, String), ()> {
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
                let my_udt = result[3].as_str().ok_or(())?;

                Ok((
                    rollup_lock.to_owned(),
                    rollup_lock_dep.to_owned(),
                    udt_lock.to_owned(),
                    my_udt.to_owned(),
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

pub async fn listen_blocks(
    block_height: u64,
    rollup_hash: &String,
) -> Result<Vec<(Vec<(Vec<u8>, String, String, Option<(String, u128)>)>, u64)>, ()> {
    let rollup_lock = Script::new_unchecked(hex::decode(rollup_hash).unwrap().into());

    //get_tip_block_number
    let now_height = match surf::post(NODE_RPC_ADDR)
        .body_json(&jsonrpc("get_tip_block_number", json!(vec![""])))
        .map_err(|_e| ())?
        .await
    {
        Ok(mut res) => match res.body_json::<Value>().await {
            Ok(mut value) => {
                let result = value["result"].take();
                result.as_u64().ok_or(())?
            }
            Err(err) => {
                println!("JSONRPC listen block height err: {:?}", err);
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
        return Ok(vec![]);
    }

    let mut blocks = vec![];

    for i in block_height..now_height {
        println!("START GOT BLOCK: {}", i);

        let mut tmp_block = vec![];

        // get block info
        if let Ok(mut res) = surf::post(NODE_RPC_ADDR)
            .body_json(&jsonrpc("get_block", json!(vec![format!("{}", i)])))
            .map_err(|_e| ())?
            .await
        {
            let result = res.body_json::<Value>().await.map_err(|_| ())?;
            let transactions = result["result"].as_array().ok_or(())?;

            for mock_tx in transactions {
                let cells = mock_tx.as_array().unwrap();

                let first_cell = cells[0].as_array().unwrap();

                let first_lock = Script::new_unchecked(
                    hex::decode(first_cell[0].as_str().unwrap()).unwrap().into(),
                );
                let first_point = first_cell[1].as_str().unwrap().to_owned();
                let first_data = hex::decode(first_cell[2].as_str().unwrap()).unwrap();

                if first_lock == rollup_lock {
                    let mut block_data = first_data;
                    let commit_cell_point = first_point;
                    let upk_cell_point =
                        cells[1].as_array().unwrap()[1].as_str().unwrap().to_owned();

                    let udt_cell = match block_data.remove(0) {
                        1u8 | 2u8 => {
                            let udt_cell = cells[2].as_array().unwrap();

                            let mut u128_bytes = [0u8; 16];
                            u128_bytes.copy_from_slice(
                                &hex::decode(udt_cell[2].as_str().unwrap()).unwrap(),
                            );
                            let amount = u128::from_le_bytes(u128_bytes);
                            let udt_cell_point = udt_cell[1].as_str().unwrap().to_owned();
                            Some((udt_cell_point, amount))
                        }
                        _ => None,
                    };

                    tmp_block.push((block_data, commit_cell_point, upk_cell_point, udt_cell));
                }
            }
        } else {
            println!("ERROR------------");
            break;
        }
        blocks.push((tmp_block, i));
    }

    Ok(blocks)
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

                for _tx in transactions {
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
    upks: Vec<u8>,
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
        .capacity(Capacity::bytes(3000).unwrap().as_u64().pack())
        .lock(rollup_lock.clone())
        .build();
    let init_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();

    let mut true_commit = vec![0u8];
    true_commit.extend_from_slice(&mut commit[..]);

    let init_outputs_data: Vec<Bytes> = vec![
        true_commit.into(),
        upks.into(),
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

    let tx_hash = tx.hash();

    let mut results = send_tx(tx.pack()).await?;

    Ok((
        results.remove(0),
        results.remove(0),
        results.remove(0),
        hex::encode(tx_hash.as_slice()),
    ))
}

pub async fn send_deposit(
    rollup_hash: &String,
    rollup_dep_hash: &String,
    success_hash: &String,
    my_udt_hash: &String,
    pre_commit_hash: &String,
    pre_upk_hash: &String,
    pre_udt_hash: &String,
    mut commit: Vec<u8>,
    upks: Vec<u8>,
    udt_amount: u128,
    my_udt_amount: u128,
) -> Result<(String, String, String, String, String), ()> {
    let rollup_lock = Script::new_unchecked(hex::decode(rollup_hash).unwrap().into());
    let rollup_dep = CellDep::new_unchecked(hex::decode(rollup_dep_hash).unwrap().into());
    let success_lock = Script::new_unchecked(hex::decode(success_hash).unwrap().into());
    let my_udt = OutPoint::new_unchecked(hex::decode(my_udt_hash).unwrap().into());

    let pre_commit = OutPoint::new_unchecked(hex::decode(pre_commit_hash).unwrap().into());
    let pre_upk = OutPoint::new_unchecked(hex::decode(pre_upk_hash).unwrap().into());
    let pre_udt = OutPoint::new_unchecked(hex::decode(pre_udt_hash).unwrap().into());

    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();

    let udt_input = CellInput::new_builder().previous_output(my_udt).build();

    let deposit_commit_input = CellInput::new_builder().previous_output(pre_commit).build();
    let deposit_upk_input = CellInput::new_builder().previous_output(pre_upk).build();
    let deposit_udt_input = CellInput::new_builder().previous_output(pre_udt).build();

    let commit_cell = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();
    let upk_cell = CellOutput::new_builder()
        .capacity(Capacity::bytes(3000).unwrap().as_u64().pack())
        .lock(rollup_lock.clone())
        .build();
    let udt_cell = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock)
        .build();
    let my_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(success_lock)
        .build();

    let mut true_commit = vec![1u8];
    true_commit.extend_from_slice(&mut commit[..]);

    let deposit_outputs_data: Vec<Bytes> = vec![
        true_commit.into(),
        upks.into(),
        udt_amount.to_le_bytes().to_vec().into(),
        my_udt_amount.to_le_bytes().to_vec().into(),
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![
            deposit_commit_input,
            deposit_upk_input,
            deposit_udt_input,
            udt_input,
        ])
        .outputs(vec![commit_cell, upk_cell, udt_cell, my_udt])
        .outputs_data(deposit_outputs_data.pack())
        .cell_dep(rollup_dep)
        .build();

    let tx_hash = tx.hash();

    let mut results = send_tx(tx.pack()).await?;

    Ok((
        results.remove(0),
        results.remove(0),
        results.remove(0),
        results.remove(0),
        hex::encode(tx_hash.as_slice()),
    ))
}

pub async fn send_withdraw(
    rollup_hash: &String,
    rollup_dep_hash: &String,
    success_hash: &String,
    pre_commit_hash: &String,
    pre_upk_hash: &String,
    pre_udt_hash: &String,
    mut commit: Vec<u8>,
    upks: Vec<u8>,
    udt_amount: u128,
    amount: u128,
) -> Result<(String, String, String, String), ()> {
    let rollup_lock = Script::new_unchecked(hex::decode(rollup_hash).unwrap().into());
    let rollup_dep = CellDep::new_unchecked(hex::decode(rollup_dep_hash).unwrap().into());
    let success_lock = Script::new_unchecked(hex::decode(success_hash).unwrap().into());

    let pre_commit = OutPoint::new_unchecked(hex::decode(pre_commit_hash).unwrap().into());
    let pre_upk = OutPoint::new_unchecked(hex::decode(pre_upk_hash).unwrap().into());
    let pre_udt = OutPoint::new_unchecked(hex::decode(pre_udt_hash).unwrap().into());

    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();

    let withdraw_commit_input = CellInput::new_builder().previous_output(pre_commit).build();
    let withdraw_upk_input = CellInput::new_builder().previous_output(pre_upk).build();
    let withdraw_udt_input = CellInput::new_builder().previous_output(pre_udt).build();

    let commit_cell = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();
    let upk_cell = CellOutput::new_builder()
        .capacity(Capacity::bytes(3000).unwrap().as_u64().pack())
        .lock(rollup_lock.clone())
        .build();
    let udt_cell = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock)
        .build();
    let my_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(success_lock)
        .build();

    let mut true_commit = vec![2u8];
    true_commit.extend_from_slice(&mut commit[..]);

    let withdraw_outputs_data: Vec<Bytes> = vec![
        true_commit.into(),
        upks.into(),
        udt_amount.to_le_bytes().to_vec().into(),
        amount.to_le_bytes().to_vec().into(),
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![
            withdraw_commit_input,
            withdraw_upk_input,
            withdraw_udt_input,
        ])
        .outputs(vec![commit_cell, upk_cell, udt_cell, my_udt])
        .outputs_data(withdraw_outputs_data.pack())
        .cell_dep(rollup_dep)
        .build();

    let tx_hash = tx.hash();

    let mut results = send_tx(tx.pack()).await?;

    Ok((
        results.remove(0),
        results.remove(0),
        results.remove(0),
        hex::encode(tx_hash.as_slice()),
    ))
}

pub async fn send_block(
    rollup_hash: &String,
    rollup_dep_hash: &String,
    pre_commit_hash: &String,
    pre_upk_hash: &String,
    mut commit: Vec<u8>,
    upks: Vec<u8>,
) -> Result<(String, String, String), ()> {
    let rollup_lock = Script::new_unchecked(hex::decode(rollup_hash).unwrap().into());
    let rollup_dep = CellDep::new_unchecked(hex::decode(rollup_dep_hash).unwrap().into());

    let pre_commit = OutPoint::new_unchecked(hex::decode(pre_commit_hash).unwrap().into());
    let pre_upk = OutPoint::new_unchecked(hex::decode(pre_upk_hash).unwrap().into());

    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();

    let commit_input = CellInput::new_builder().previous_output(pre_commit).build();
    let upk_input = CellInput::new_builder().previous_output(pre_upk).build();

    let commit_cell = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock.clone())
        .build();
    let upk_cell = CellOutput::new_builder()
        .capacity(Capacity::bytes(3000).unwrap().as_u64().pack())
        .lock(rollup_lock.clone())
        .build();

    let mut true_commit = vec![3u8];
    true_commit.extend_from_slice(&mut commit[..]);

    let outputs_data: Vec<Bytes> = vec![true_commit.into(), upks.into()];

    let tx = TransactionBuilder::default()
        .inputs(vec![commit_input, upk_input])
        .outputs(vec![commit_cell, upk_cell])
        .outputs_data(outputs_data.pack())
        .cell_dep(rollup_dep)
        .build();

    let tx_hash = tx.hash();
    println!("Send transaction: {}", hex::encode(tx_hash.as_slice()));

    let mut results = send_tx(tx.pack()).await?;

    Ok((
        results.remove(0),
        results.remove(0),
        hex::encode(tx_hash.as_slice()),
    ))
}

async fn send_tx(tx: TransactionView) -> Result<Vec<String>, ()> {
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
            Ok(value) => Ok(value["result"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect()),
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
