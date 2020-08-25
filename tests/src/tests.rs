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
    let success_lock_script_dep = CellDep::new_builder().out_point(success_point).build();

    let rollup_bin: Bytes = Loader::default().load_binary("asvc_rollup");
    let rollup_point = context.deploy_cell(rollup_bin);
    let rollup_script_args: Bytes = [0u8; 1].to_vec().into();
    let rollup_lock_script = context
        .build_script(&rollup_point, rollup_script_args)
        .expect("script");
    let rollup_lock_script_dep = CellDep::new_builder().out_point(rollup_point).build();
    println!("over deploy contract...");

    // ----- Init State -------
    println!("start init state...");

    println!("over init state...");
    // ----- End Init State ---

    // ----- Deposit ----------
    println!("start deposit...");
    // inputs udt demo
    let input_ckb = Capacity::bytes(1000).unwrap().as_u64();
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

    println!("over deposit...");
    // ----- End Deposit -------

    // ----- POST block -------
    println!("start post block...");

    println!("over post block...");
    // ----- End POST block ---

    // ----- Withdraw ---------
    println!("start withdraw...");

    println!("over withdraw...");
    // ----- End Withdraw -----

    println!("all over.");
}
