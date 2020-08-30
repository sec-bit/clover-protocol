use ckb_zkp::math::{Field, FromBytes, PairingEngine, ToBytes, Zero};
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, Proof, UpdateKey, VerificationKey,
};
use core::ops::{Add, Mul, Sub};

use crate::transaction::{u128_to_fr, Transaction, TxType, ACCOUNT_SIZE};
use crate::{vec, String, Vec};

pub struct CellUpks<E: PairingEngine> {
    pub vk: VerificationKey<E>,
    pub omega: E::Fr,
    pub upks: Vec<UpdateKey<E>>,
}

impl<E: PairingEngine> CellUpks<E> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.vk.write(&mut bytes).unwrap();
        self.omega.write(&mut bytes).unwrap();
        (self.upks.len() as u32).write(&mut bytes).unwrap();
        for u in &self.upks {
            u.write(&mut bytes).unwrap();
        }
        bytes
    }

    pub fn from_bytes(mut s: &[u8]) -> Result<Self, ()> {
        let vk = VerificationKey::read(&mut s).map_err(|_| ())?;
        let omega = E::Fr::read(&mut s).map_err(|_| ())?;

        let n = u32::read(&mut s).map_err(|_| ())?;
        let mut upks = Vec::new();
        for _ in 0..n {
            upks.push(UpdateKey::read(&mut s).map_err(|_| ())?);
        }

        Ok(Self { vk, omega, upks })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Block<E: PairingEngine> {
    pub block_height: u32,
    pub commit: Commitment<E>,
    pub proof: Proof<E>,
    pub new_commit: Commitment<E>,
    pub txs: Vec<Transaction<E>>,
}

impl<E: PairingEngine> Block<E> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.block_height.write(&mut bytes).unwrap();
        self.commit.write(&mut bytes).unwrap();
        self.new_commit.write(&mut bytes).unwrap();
        self.proof.write(&mut bytes).unwrap();
        (self.txs.len() as u32).write(&mut bytes).unwrap();
        for tx in &self.txs {
            tx.write(&mut bytes).unwrap();
        }
        bytes
    }

    pub fn from_bytes(mut s: &[u8]) -> Result<Self, ()> {
        let block_height = u32::read(&mut s).map_err(|_| ())?;

        let commit = Commitment::read(&mut s).map_err(|_| ())?;
        let new_commit = Commitment::read(&mut s).map_err(|_| ())?;
        let proof = Proof::read(&mut s).map_err(|_| ())?;
        let n = u32::read(&mut s).map_err(|_| ())?;
        let mut txs = Vec::new();
        for _ in 0..n {
            txs.push(Transaction::read(&mut s).map_err(|_| ())?);
        }

        Ok(Self {
            block_height,
            commit,
            new_commit,
            proof,
            txs,
        })
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    pub fn from_hex(s: &str) -> Result<Self, ()> {
        let v: Vec<u8> = hex::decode(s).map_err(|_| ())?;
        Self::from_bytes(&v[..])
    }

    pub fn verify(&self, cell_upks: &CellUpks<E>) -> Result<i128, String> {
        if cell_upks.upks.len() != ACCOUNT_SIZE {
            return Err(String::from("BLOCK_VERIFY: Upk length"));
        }

        let mut points2prove: Vec<u32> = Vec::new();

        // - Delta and point_values should be calculated during txs traversing.
        // - Delta can be accumulated, cause commit updating process is additive and sequence independent.
        // - Point_value calculated once for each point that is needed to be proved.
        // - A specific account's balance should remain identical.
        // - While an account become a transfer-from, his up-to-that-tx balance should be verified suffient.
        // - In one transfer, a fransfer-to account's nonce remain unchanged.
        // - L2 block height should be strictly incremental by one.
        // - Each account only need to execute proof-verifying once.

        // map[point](0: delta, 1: balance_change, 2: Option<proof_param>,
        //            3: current_nonce, 4: origin_balance)
        // the "origin" means the value is to be proved during this verification.
        let mut table: Vec<(Option<E::Fr>, i128, Option<E::Fr>, u32, u128)> =
            vec![(None, 0, None, 0, 0); ACCOUNT_SIZE];
        // aggregate the overall capital changing of the block

        let mul160: E::Fr = E::Fr::from(2).pow(&[160]);
        let mul128: E::Fr = E::Fr::from(2).pow(&[128]);

        let mut overall_change: i128 = 0;

        for tx in &self.txs {
            match tx.tx_type {
                // A block submitted by user only contains Deposit and Withdraw transactions.
                TxType::Deposit(to, amount) => {
                    overall_change += amount as i128;

                    match table[to as usize].0 {
                        None => {
                            points2prove.push(to);
                            table[to as usize] = (
                                Some(E::Fr::zero().add(&u128_to_fr::<E>(amount))),
                                amount as i128,
                                Some(tx.point_value()),
                                tx.nonce,
                                tx.balance,
                            );
                        }
                        Some(_) => {
                            table[to as usize].0 =
                                Some(table[to as usize].0.unwrap().add(&u128_to_fr::<E>(amount)));
                            table[to as usize].1 += amount as i128;
                        }
                    }
                }
                TxType::Withdraw(from, amount) => {
                    overall_change -= amount as i128;

                    match table[from as usize].0 {
                        None => {
                            points2prove.push(from);
                            table[from as usize] = (
                                Some(E::Fr::zero().sub(&u128_to_fr::<E>(amount))),
                                -(amount as i128),
                                Some(tx.point_value()),
                                tx.nonce,
                                tx.balance,
                            )
                        }
                        Some(_) => {
                            table[from as usize].0 = Some(
                                table[from as usize]
                                    .0
                                    .unwrap()
                                    .sub(&u128_to_fr::<E>(amount)),
                            );
                            table[from as usize].1 -= amount as i128;
                        }
                    }
                    // balance sufficiency check
                    if table[from as usize].1 < 0 {
                        if table[from as usize].4 < (-table[from as usize].1 as u128) {
                            return Err(String::from("BLOCK_VERIFY: balance check failure"));
                        }
                    }
                }
                // A block submitted by L2 service only contains Transfer and Register transactions.
                TxType::Transfer(from, to, amount) => {
                    match table[from as usize].0 {
                        None => {
                            points2prove.push(from);
                            table[from as usize] = (
                                Some(mul128.sub(&u128_to_fr::<E>(amount))),
                                -(amount as i128),
                                Some(tx.point_value()),
                                tx.nonce,
                                tx.balance,
                            );
                        }
                        Some(_) => match table[from as usize].2 {
                            None => {
                                points2prove.push(from);
                                table[from as usize] = (
                                    Some(
                                        table[from as usize]
                                            .0
                                            .unwrap()
                                            .add(&mul128)
                                            .sub(&u128_to_fr::<E>(amount)),
                                    ),
                                    table[from as usize].1 - (amount as i128),
                                    Some(tx.point_value()),
                                    tx.nonce,
                                    tx.balance,
                                );
                            }
                            Some(_) => {
                                if tx.nonce - table[from as usize].3 != 1 {
                                    return Err(String::from("BLOCK_VERIFY: nonce invalid"));
                                }
                                table[from as usize].0 = Some(
                                    table[from as usize]
                                        .0
                                        .unwrap()
                                        .add(&mul128)
                                        .sub(&u128_to_fr::<E>(amount)),
                                );
                                table[from as usize].1 -= amount as i128;
                                table[from as usize].3 = tx.nonce;
                            }
                        },
                    }
                    // balance sufficiency check
                    if table[from as usize].1 < 0 {
                        if table[from as usize].4 < (-table[from as usize].1 as u128) {
                            return Err(String::from("BLOCK_VERIFY: balance invalid"));
                        }
                    }
                    match table[to as usize].0 {
                        None => {
                            // In a Transfer, the nonce of transfer-to account remains unchanged.
                            table[to as usize] =
                                (Some(u128_to_fr::<E>(amount)), amount as i128, None, 0, 0);
                        }
                        Some(_) => {
                            table[to as usize].0 =
                                Some(table[to as usize].0.unwrap().add(&u128_to_fr::<E>(amount)));
                            table[to as usize].1 += amount as i128;
                        }
                    }
                }
                TxType::Register(to) => {
                    // A user must be registered to got paid.
                    // So the Registration should happen on a new user.
                    table[to as usize] = (
                        Some(tx.addr.mul(&mul160).add(&mul128)),
                        0,
                        Some(tx.point_value()),
                        tx.nonce,
                        tx.balance,
                    );
                }
            }
        }

        let mut point_values = Vec::new();
        let mut tmp_commit = self.commit.clone();

        for point in &points2prove {
            point_values.push(table[*point as usize].2.unwrap());
            tmp_commit = update_commit(
                &tmp_commit,
                table[*point as usize].0.unwrap(),
                *point as u32,
                &cell_upks.upks[*point as usize],
                cell_upks.omega,
                ACCOUNT_SIZE,
            )
            .map_err(|_| String::from("BLOCK_VERIFY: update commit failure!"))?;
        }

        verify_pos::<E>(
            &cell_upks.vk,
            &self.commit,
            point_values,
            points2prove,
            &self.proof,
            cell_upks.omega,
        )
        .map_err(|_| String::from("BLOCK_VERIFY: verify pos failure!"))?;

        Ok(overall_change)
    }
}
