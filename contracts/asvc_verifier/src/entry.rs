use alloc::vec::Vec;
use core::result::Result;

use ckb_std::{
    ckb_constants::Source,
    debug,
    error::SysError,
    high_level::{load_cell_data, load_cell_lock_hash, load_cell_type_hash, load_script_hash},
};

use asvc_rollup::block::{Block, CellUpks};
use ckb_zkp::curve::bn_256::Bn_256;

use crate::error::Error;

// use simple UDT length
const UDT_LEN: usize = 16; // u128
const BLOCK_CELL: usize = 0;
const UPK_CELL: usize = 1;

pub fn main() -> Result<(), Error> {
    // load now commit
    let now_commit = match load_cell_data(BLOCK_CELL, Source::Output) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };

    let now_com_lock = load_cell_lock_hash(BLOCK_CELL, Source::Output)?;

    if now_commit.len() == 0 {
        return Err(Error::LengthNotEnough);
    }

    let now_upk = match load_cell_data(UPK_CELL, Source::Output) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };
    let now_upk_lock = load_cell_lock_hash(UPK_CELL, Source::Output)?;

    let pre_commit = match load_cell_data(BLOCK_CELL, Source::Input) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };
    let pre_com_lock = load_cell_lock_hash(BLOCK_CELL, Source::Input)?;

    let pre_upk = match load_cell_data(UPK_CELL, Source::Input) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };
    let pre_upk_lock = load_cell_lock_hash(UPK_CELL, Source::Input)?;

    if now_upk != pre_upk {
        return Err(Error::Upk);
    }

    let self_script_hash = load_script_hash().unwrap();

    if self_script_hash != now_com_lock {
        return Err(Error::Verify);
    }
    if (now_com_lock != now_upk_lock)
        || (now_com_lock != pre_com_lock)
        || (now_com_lock != pre_upk_lock)
    {
        return Err(Error::Verify);
    }

    let op = now_commit[0];
    match op {
        1u8 => {
            // DEPOSIT
            //
            // input0 => pre_commit
            // input1 => upk
            // input2 => pre_udt_pool
            // input3..n-1 => udt_unspend
            // output0 => now_commit
            // output1 => upk
            // output2 => now_udt_pool
            // output3..n-1 => udt_change

            // 2. pre udt amount in pool.
            debug!("DEPOSIT");
            let pre_amount = match load_cell_data(2, Source::Input) {
                Ok(data) => {
                    let mut buf = [0u8; UDT_LEN];
                    if data.len() != UDT_LEN {
                        return Err(Error::Encoding);
                    }

                    buf.copy_from_slice(&data);
                    u128::from_le_bytes(buf)
                }
                Err(err) => return Err(err.into()),
            };
            let pre_amount_lock = load_cell_lock_hash(2, Source::Input)?;
            let pre_amount_type = load_cell_type_hash(2, Source::Input)?;

            // 3. inputs udt deposit.
            let mut deposit_amount: u128 = 0;
            let mut deposit_buf = [0u8; UDT_LEN];

            for i in 3.. {
                let data = match load_cell_data(i, Source::Input) {
                    Ok(data) => data,
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                };

                let udt_type = load_cell_type_hash(i, Source::Input)?;

                if udt_type != pre_amount_type {
                    continue;
                }

                if data.len() != UDT_LEN {
                    return Err(Error::Encoding);
                }

                deposit_buf.copy_from_slice(&data);
                deposit_amount += u128::from_le_bytes(deposit_buf);
            }

            // 4. output udt amount.
            let now_amount = match load_cell_data(2, Source::Output) {
                Ok(data) => {
                    let mut buf = [0u8; UDT_LEN];
                    if data.len() != UDT_LEN {
                        return Err(Error::Encoding);
                    }

                    buf.copy_from_slice(&data);
                    u128::from_le_bytes(buf)
                }
                Err(err) => return Err(err.into()),
            };
            let now_amount_lock = load_cell_lock_hash(2, Source::Output)?;
            let now_amount_type = load_cell_type_hash(2, Source::Input)?;

            if (pre_amount_lock != now_amount_lock) || (pre_amount_type != now_amount_type) {
                return Err(Error::Amount);
            }
            if now_com_lock != pre_amount_lock {
                return Err(Error::Verify);
            }

            // 5. change outputs udt.
            let mut change_amount: u128 = 0;
            let mut change_buf = [0u8; UDT_LEN];

            for i in 3.. {
                let data = match load_cell_data(i, Source::Output) {
                    Ok(data) => data,
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                };

                let udt_type = load_cell_type_hash(i, Source::Output)?;

                if udt_type != now_amount_type {
                    continue;
                }

                if data.len() != UDT_LEN {
                    return Err(Error::Encoding);
                }

                change_buf.copy_from_slice(&data);
                change_amount += u128::from_le_bytes(change_buf);
            }

            if now_amount < pre_amount {
                return Err(Error::Amount);
            }

            // 6. why not check UDT name is equal, because UDT's type will check it.
            if now_amount + change_amount != pre_amount + deposit_amount {
                return Err(Error::Amount);
            }

            // 7. verify commit info.
            verify(
                pre_commit,
                now_commit,
                now_upk,
                deposit_amount - change_amount,
                true,
            )
        }
        2u8 => {
            // WITHDRAW
            //
            // input0 => pre_commit
            // input1 => upk
            // input2 => pre_udt_pool
            // output0 => now_commit
            // output1 => upk
            // output2 => now_udt_pool
            // output3..n-1 => udt_unspend

            // 2. pre udt amount in pool.
            debug!("WITHDRAW");

            let pre_amount = match load_cell_data(2, Source::Input) {
                Ok(data) => {
                    let mut buf = [0u8; UDT_LEN];
                    if data.len() != UDT_LEN {
                        return Err(Error::Encoding);
                    }

                    buf.copy_from_slice(&data);
                    u128::from_le_bytes(buf)
                }
                Err(err) => return Err(err.into()),
            };
            let pre_amount_lock = load_cell_lock_hash(2, Source::Input)?;
            let pre_amount_type = load_cell_type_hash(2, Source::Input)?;

            // 3. now udt pool amount.
            let now_amount = match load_cell_data(2, Source::Output) {
                Ok(data) => {
                    let mut buf = [0u8; UDT_LEN];
                    if data.len() != UDT_LEN {
                        return Err(Error::Encoding);
                    }

                    buf.copy_from_slice(&data);
                    u128::from_le_bytes(buf)
                }
                Err(err) => return Err(err.into()),
            };
            let now_amount_lock = load_cell_lock_hash(2, Source::Output)?;
            let now_amount_type = load_cell_type_hash(2, Source::Input)?;

            if (pre_amount_lock != now_amount_lock) || (pre_amount_type != now_amount_type) {
                return Err(Error::Amount);
            }

            if now_com_lock != pre_amount_lock {
                return Err(Error::Verify);
            }

            // 4. outputs udt.
            let mut withdraw_amount: u128 = 0;
            let mut withdraw_buf = [0u8; UDT_LEN];

            for i in 3.. {
                let data = match load_cell_data(i, Source::Output) {
                    Ok(data) => data,
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                };

                let udt_type = load_cell_type_hash(i, Source::Output)?;

                if udt_type != now_amount_type {
                    continue;
                }

                if data.len() != UDT_LEN {
                    return Err(Error::Encoding);
                }

                withdraw_buf.copy_from_slice(&data);
                withdraw_amount += u128::from_le_bytes(withdraw_buf);
            }

            // 5. check amount.
            if now_amount + withdraw_amount != pre_amount {
                return Err(Error::Amount);
            }

            // 6. verify commit.
            verify(pre_commit, now_commit, now_upk, withdraw_amount, false)
        }
        3u8 => {
            // POST BLOCK
            //
            // input0 => pre_commit
            // input1 => upk
            // output0 => now_commit
            // output1 => upk

            debug!("POST BLOCK");
            // presence of any other cells with the same lock script
            // (to be precise, udt pool cells) is illegal.
            for i in 2.. {
                match load_cell_lock_hash(i, Source::Input) {
                    Ok(lock) => {
                        if lock == self_script_hash {
                            return Err(Error::Amount);
                        }
                    }
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                }
                match load_cell_lock_hash(i, Source::Output) {
                    Ok(lock) => {
                        if lock == self_script_hash {
                            return Err(Error::Amount);
                        }
                    }
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            // post block proof
            verify(pre_commit, now_commit, now_upk, 0, false)
        }
        _ => Err(Error::Encoding),
    }
}

fn verify(
    mut pre: Vec<u8>,
    mut now: Vec<u8>,
    upk: Vec<u8>,
    change: u128,
    is_add: bool,
) -> Result<(), Error> {
    debug!("change: {}{}", if is_add { "+" } else { "-" }, change);

    pre.remove(0);
    now.remove(0);

    let pre_block = Block::<Bn_256>::from_bytes(&pre[..]).unwrap();
    let now_block = Block::<Bn_256>::from_bytes(&now[..]).unwrap();

    debug!("pre & now block ok");
    if pre_block.new_commit != now_block.commit {
        return Err(Error::Verify);
    }
    debug!("pre & now block is eq ok");

    let cell_upks = CellUpks::<Bn_256>::from_bytes(&upk[..]).unwrap();

    let mut udt_change = change as i128;
    if !is_add {
        udt_change = -udt_change;
    }

    match now_block.verify(&cell_upks) {
        Ok(r) => {
            if r == udt_change {
                return Ok(());
            }
            debug!(
                "udt amount change mismatched :on chain {}, off chain: {}",
                udt_change, r
            );
            return Err(Error::Amount);
        }
        _ => {
            debug!("verify failure");
            return Err(Error::Verify);
        }
    };
}
