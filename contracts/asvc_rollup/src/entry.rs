use alloc::vec::Vec;
use core::result::Result;

use ckb_std::{
    ckb_constants::Source,
    debug,
    error::SysError,
    high_level::{load_cell_data, load_cell_lock_hash},
};

use crate::error::Error;

// use simple UDT length
const UDT_LEN: usize = 16; // u128

pub fn main() -> Result<(), Error> {
    // load now commit
    let now_commit = match load_cell_data(0, Source::Output) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };

    if now_commit.len() == 0 {
        return Err(Error::LengthNotEnough);
    }

    let now_upk = match load_cell_data(1, Source::Output) {
        Ok(data) => data,
        Err(err) => return Err(err.into()),
    };

    let op = now_commit[0];

    match op {
        0u8 => {
            // Init state
            Ok(())
        }
        1u8 => {
            // DEPOSIT
            //
            // input1 => pre_commit
            // input2 => upk
            // input3 => pre_udt_pool
            // input4..n => udt_unspend
            // output1 => now_commit
            // output2 => upk
            // output3 => now_udt_pool
            // output4..n => udt_change

            // 1. commit cell
            let pre_commit = match load_cell_data(0, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            let pre_upk = match load_cell_data(1, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            if now_upk != pre_upk {
                return Err(Error::Upk);
            }

            // 2. pre udt amount in pool.
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

            // 3. inputs udt deposit.
            let mut deposit_amount: u128 = 0;
            let mut deposit_buf = [0u8; UDT_LEN];

            for i in 3.. {
                let data = match load_cell_data(i, Source::Input) {
                    Ok(data) => data,
                    Err(SysError::IndexOutOfBound) => break,
                    Err(err) => return Err(err.into()),
                };

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

            let int_lock = load_cell_lock_hash(2, Source::Input)?;
            let out_lock = load_cell_lock_hash(2, Source::Output)?;
            if int_lock != out_lock {
                return Err(Error::Amount);
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
            verify(pre_commit, now_commit, now_upk)
        }
        2u8 => {
            // WITHDRAW
            //
            // input1 => pre_commit
            // input2 => pre_upk
            // input3 => pre_udt_pool
            // output1 => now_commit
            // output2 => now_udt_pool
            // output3..n => udt_unspend

            // 1. previous commit cell
            let pre_commit = match load_cell_data(0, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            let pre_upk = match load_cell_data(1, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            if now_upk != pre_upk {
                return Err(Error::Upk);
            }

            // 2. pre udt amount in pool.
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
            let int_lock = load_cell_lock_hash(2, Source::Input)?;
            let out_lock = load_cell_lock_hash(2, Source::Output)?;
            if int_lock != out_lock {
                return Err(Error::Amount);
            }
            let int_lock = load_cell_lock_hash(2, Source::Input)?;
            let out_lock = load_cell_lock_hash(2, Source::Output)?;
            if int_lock != out_lock {
                return Err(Error::Amount);
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
            verify(pre_commit, now_commit, now_upk)
        }
        3u8 => {
            // POST BLOCK
            //
            // input1 => pre_commit
            // output1 => now_commit

            // commit cell
            let pre_commit = match load_cell_data(0, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            let pre_upk = match load_cell_data(1, Source::Input) {
                Ok(data) => data,
                Err(err) => return Err(err.into()),
            };
            if now_upk != pre_upk {
                return Err(Error::Upk);
            }

            // post block proof
            verify(pre_commit, now_commit, now_upk)
        }
        _ => Err(Error::Encoding),
    }
}

fn verify(pre: Vec<u8>, now: Vec<u8>, upk: Vec<u8>) -> Result<(), Error> {
    debug!("pre: {:?}", pre);
    debug!("now: {:?}", now);
    debug!("upk: {:?}", upk);

    Ok(())
}
