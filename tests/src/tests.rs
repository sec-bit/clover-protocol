use super::*;
use ckb_testtool::{builtin::ALWAYS_SUCCESS, context::Context};
use ckb_tool::ckb_types::{
    bytes::Bytes,
    core::{Capacity, TransactionBuilder},
    packed::*,
    prelude::*,
};

const MAX_CYCLES: u64 = 10_000_000;

#[test]
fn test_asvc() {
    // deploy contract
    let mut context = Context::default();

    println!("start deploy contract...");
    let success_point = context.deploy_cell(ALWAYS_SUCCESS.clone());
    let success_lock_script = context
        .build_script(&success_point, Default::default())
        .expect("script");
    let _success_lock_script_dep = CellDep::new_builder().out_point(success_point).build();

    let rollup_bin: Bytes = Loader::default().load_binary("asvc_rollup");
    let rollup_point = context.deploy_cell(rollup_bin);
    let rollup_script_args: Bytes = [0u8; 1].to_vec().into();
    let rollup_lock_script = context
        .build_script(&rollup_point, rollup_script_args)
        .expect("script");
    let rollup_lock_script_dep = CellDep::new_builder().out_point(rollup_point).build();
    println!("over deploy contract...");

    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();

    // ----- Init State -------
    println!("start init state...");
    let init_output_commit = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let init_upk = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let init_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let init_outputs_data: Vec<Bytes> = vec![
        vec![0u8, 2u8, 3u8, 4u8, 5u8, 6u8].into(), // commits & txs
        vec![8u8, 8u8, 8u8].into(),                // upk
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
        .cell_dep(rollup_lock_script_dep.clone())
        .build();
    let tx = context.complete_tx(tx);

    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("over init state: {}...", cycles);
    // ----- End Init State ---

    // ----- Deposit ----------
    println!("start deposit...");
    // inputs udt demo
    let udt_input_out_point = context.create_cell(
        CellOutput::new_builder()
            .capacity(input_ckb.pack())
            .lock(success_lock_script.clone())
            .build(),
        //.type_(Some(sudt_script.clone()).pack()) Type will use UDT
        100u128.to_le_bytes().to_vec().into(),
    );
    let udt_input = CellInput::new_builder()
        .previous_output(udt_input_out_point)
        .build();

    let deposit_commit_input = CellInput::new_builder()
        .previous_output(context.create_cell(
            init_output_commit,
            vec![0u8, 2u8, 3u8, 4u8, 5u8, 6u8].into(),
        ))
        .build();
    let deposit_upk_input = CellInput::new_builder()
        .previous_output(context.create_cell(init_upk, vec![8u8, 8u8, 8u8].into()))
        .build();
    let deposit_udt_input = CellInput::new_builder()
        .previous_output(context.create_cell(init_udt, 0u128.to_le_bytes().to_vec().into()))
        .build();

    let deposit_commit = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let deposit_upk = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let deposit_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();

    let deposit_outputs_data: Vec<Bytes> = vec![
        vec![1u8, 2u8, 3u8, 4u8, 5u8, 6u8].into(), // commits & txs
        vec![8u8, 8u8, 8u8].into(),                // upk
        100u128.to_le_bytes().to_vec().into(),     // new amount
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![
            deposit_commit_input,
            deposit_upk_input,
            deposit_udt_input,
            udt_input,
        ])
        .outputs(vec![
            deposit_commit.clone(),
            deposit_upk.clone(),
            deposit_udt.clone(),
        ])
        .outputs_data(deposit_outputs_data.pack())
        .cell_dep(rollup_lock_script_dep.clone())
        .build();
    let tx = context.complete_tx(tx);

    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("over deposit: {}...", cycles);
    // ----- End Deposit -------

    // ----- POST block -------
    println!("start post block...");
    let block_commit_input = CellInput::new_builder()
        .previous_output(
            context.create_cell(deposit_commit, vec![1u8, 2u8, 3u8, 4u8, 5u8, 6u8].into()),
        )
        .build();
    let block_upk_input = CellInput::new_builder()
        .previous_output(context.create_cell(deposit_upk, vec![8u8, 8u8, 8u8].into()))
        .build();
    let block_commit = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let block_upk = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();

    let block_outputs_data: Vec<Bytes> = vec![
        vec![3u8, 2u8, 3u8, 4u8, 5u8, 6u8].into(), // commits & txs
        vec![8u8, 8u8, 8u8].into(),                // upk
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![block_commit_input, block_upk_input])
        .outputs(vec![block_commit.clone(), block_upk.clone()])
        .outputs_data(block_outputs_data.pack())
        .cell_dep(rollup_lock_script_dep.clone())
        .build();
    let tx = context.complete_tx(tx);

    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("over post block: {}...", cycles);
    // ----- End POST block ---

    // ----- Withdraw ---------
    println!("start withdraw...");
    let withdraw_commit_input = CellInput::new_builder()
        .previous_output(
            context.create_cell(block_commit, vec![3u8, 2u8, 3u8, 4u8, 5u8, 6u8].into()),
        )
        .build();
    let withdraw_upk_input = CellInput::new_builder()
        .previous_output(context.create_cell(block_upk, vec![8u8, 8u8, 8u8].into()))
        .build();
    let withdraw_udt_input = CellInput::new_builder()
        .previous_output(context.create_cell(deposit_udt, 100u128.to_le_bytes().to_vec().into()))
        .build();

    let withdraw_commit = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let withdraw_upk = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();
    let withdraw_udt = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(rollup_lock_script.clone())
        .build();

    let withdraw_udt_1 = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(success_lock_script.clone())
        .build();

    let withdraw_udt_2 = CellOutput::new_builder()
        .capacity(input_ckb.pack())
        .lock(success_lock_script.clone())
        .build();

    let withdraw_outputs_data: Vec<Bytes> = vec![
        vec![2u8, 2u8, 3u8, 4u8, 5u8, 6u8].into(), // commits & txs
        vec![8u8, 8u8, 8u8].into(),                // upk
        40u128.to_le_bytes().to_vec().into(),      // new amount
        50u128.to_le_bytes().to_vec().into(),      // new amount
        10u128.to_le_bytes().to_vec().into(),      // new amount
    ];

    let tx = TransactionBuilder::default()
        .inputs(vec![
            withdraw_commit_input,
            withdraw_upk_input,
            withdraw_udt_input,
        ])
        .outputs(vec![
            withdraw_commit,
            withdraw_upk,
            withdraw_udt,
            withdraw_udt_1,
            withdraw_udt_2,
        ])
        .outputs_data(withdraw_outputs_data.pack())
        .cell_dep(rollup_lock_script_dep)
        .build();
    let tx = context.complete_tx(tx);

    let cycles = context
        .verify_tx(&tx, MAX_CYCLES)
        .expect("pass verification");
    println!("over withdraw: {}...", cycles);
    // ----- End Withdraw -----

    println!("all over.");
}
