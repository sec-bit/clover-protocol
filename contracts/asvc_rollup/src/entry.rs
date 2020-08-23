use alloc::{vec, vec::Vec};
use core::result::Result;

use ckb_std::{
    ckb_types::{bytes::Bytes, prelude::*},
    debug,
    high_level::{load_script, load_tx_hash},
};

use crate::error::Error;

pub fn main() -> Result<(), Error> {
    // remove below examples and write your code here

    let script = load_script()?;
    let args: Bytes = script.args().unpack();
    debug!("script args is {:?}", args);

    let tx_hash = load_tx_hash()?;
    debug!("tx hash is {:?}", tx_hash);

    let _buf: Vec<_> = vec![0u8; 32];

    // TODO verify L2 block.

    // TODO update L2 commitment.

    Ok(())
}
