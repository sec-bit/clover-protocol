use ckb_zkp::math::{Field, FromBytes, PairingEngine, ToBytes, Zero};
use ckb_zkp::scheme::asvc::{
    update_commit, verify_pos, Commitment, Proof, UpdateKey, VerificationKey,
};
use core::ops::{Add, Mul, Sub};

use crate::transaction::{u128_to_fr, Transaction, TxType, ACCOUNT_SIZE};
use crate::{vec, String, Vec};

#[derive(Clone)]
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

    /// Traverse the transactions in the block, and examine the validity of each transaction.
    ///
    /// If success, returns a tuple containing the (income, outcome) of all the transactions.
    /// Income only comes from deposit transactions, while outcome only comes from withdraw transactions.
    /// The income and outcome reflect the capital change of UDT pool.
    pub fn verify(&self, cell_upks: &CellUpks<E>) -> Result<(u128, u128), String> {
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
        #[derive(Clone)]
        pub struct Tmp<E: PairingEngine> {
            /// commit variation through transactions, additive
            pub delta: Option<E::Fr>,
            pub income: u128,
            pub outcome: u128,
            /// original point value for proving, only calculated once.
            pub point_value: Option<E::Fr>,
            /// nonce of the current transaction.
            pub cur_nonce: u32,
            /// original balance for proving.
            pub balance: u128,
        }
        let mut table: Vec<Tmp<E>> = vec![
            Tmp {
                delta: None,
                income: 0,
                outcome: 0,
                point_value: None,
                cur_nonce: 0,
                balance: 0
            };
            ACCOUNT_SIZE
        ];

        let mul160: E::Fr = E::Fr::from(2).pow(&[160]);
        let mul128: E::Fr = E::Fr::from(2).pow(&[128]);

        // aggregate the overall capital changing of the block
        // let mut overall_change: i128 = 0;
        let mut incomes = 0_u128;
        let mut outcomes = 0_u128;

        #[cfg(feature = "std")]
        println!("starting dark magic...(off-chain block.verify)");
        for tx in &self.txs {
            match tx.tx_type {
                // A block submitted by user only contains Deposit and Withdraw transactions.
                TxType::Deposit(to, amount) => {
                    incomes += amount;

                    match table[to as usize].delta {
                        None => {
                            points2prove.push(to);
                            table[to as usize] = Tmp {
                                delta: Some(E::Fr::zero().add(&u128_to_fr::<E>(amount))),
                                income: amount,
                                outcome: 0,
                                point_value: Some(tx.point_value()),
                                cur_nonce: tx.nonce,
                                balance: tx.balance,
                            };

                            #[cfg(feature = "std")]
                            println!("{} deposit for the 1st time!", to);
                        }
                        Some(prev_delta) => {
                            table[to as usize].delta =
                                Some(prev_delta.add(&u128_to_fr::<E>(amount)));
                            table[to as usize].income += amount;

                            #[cfg(feature = "std")]
                            println!("{} deposit again!", to);
                        }
                    }
                }
                TxType::Withdraw(from, amount) => {
                    outcomes += amount;

                    match table[from as usize].delta {
                        None => {
                            points2prove.push(from);
                            table[from as usize] = Tmp {
                                delta: Some(E::Fr::zero().sub(&u128_to_fr::<E>(amount))),
                                income: 0,
                                outcome: amount,
                                point_value: Some(tx.point_value()),
                                cur_nonce: tx.nonce,
                                balance: tx.balance,
                            };
                            #[cfg(feature = "std")]
                            println!("{} withdraw for the 1st time!", from);
                        }
                        Some(prev_delta) => {
                            table[from as usize].delta =
                                Some(prev_delta.sub(&u128_to_fr::<E>(amount)));
                            table[from as usize].outcome += amount;

                            #[cfg(feature = "std")]
                            println!("{} withdraw again!", from);
                        }
                    }
                    // balance sufficiency check
                    if let None = (table[from as usize].balance + table[from as usize].income)
                        .checked_sub(table[from as usize].outcome)
                    {
                        return Err(String::from("BLOCK_VERIFY: balance invalid"));
                    }
                }
                // A block submitted by L2 service only contains Transfer and Register transactions.
                TxType::Transfer(from, to, amount) => {
                    match table[from as usize].delta {
                        None => {
                            points2prove.push(from);
                            table[from as usize] = Tmp {
                                delta: Some(mul128.sub(&u128_to_fr::<E>(amount))),
                                income: 0,
                                outcome: amount,
                                point_value: Some(tx.point_value()),
                                cur_nonce: tx.nonce,
                                balance: tx.balance,
                            };

                            #[cfg(feature = "std")]
                            println!("{} transfer-from for the 1st time!", from);
                        }
                        Some(prev_delta) => match table[from as usize].point_value {
                            None => {
                                points2prove.push(from);
                                table[from as usize] = Tmp {
                                    delta: Some(
                                        prev_delta.add(&mul128).sub(&u128_to_fr::<E>(amount)),
                                    ),
                                    income: table[from as usize].income,
                                    outcome: table[from as usize].outcome + amount,
                                    point_value: Some(tx.point_value()),
                                    cur_nonce: tx.nonce,
                                    balance: tx.balance,
                                };

                                #[cfg(feature = "std")]
                                println!(
                                    "{} has presented but transfer-from for the 1st time!",
                                    from
                                );
                            }
                            Some(_) => {
                                if tx.nonce - table[from as usize].cur_nonce != 1 {
                                    return Err(String::from("BLOCK_VERIFY: nonce invalid"));
                                }
                                table[from as usize].delta = Some(
                                    table[from as usize]
                                        .delta
                                        .unwrap()
                                        .add(&mul128)
                                        .sub(&u128_to_fr::<E>(amount)),
                                );
                                table[from as usize].outcome += amount;
                                table[from as usize].cur_nonce = tx.nonce;

                                #[cfg(feature = "std")]
                                println!("{} has presented and transfer-from again!", from);
                            }
                        },
                    }
                    // transfer-from balance sufficiency check
                    if let None = (table[from as usize].balance + table[from as usize].income)
                        .checked_sub(table[from as usize].outcome)
                    {
                        return Err(String::from("BLOCK_VERIFY: balance invalid"));
                    }

                    match table[to as usize].delta {
                        None => {
                            // In a Transfer, the nonce of transfer-to account remains unchanged.
                            table[to as usize] = Tmp {
                                delta: Some(u128_to_fr::<E>(amount)),
                                income: amount,
                                outcome: 0,
                                point_value: None,
                                cur_nonce: 0,
                                balance: 0,
                            };

                            #[cfg(feature = "std")]
                            println!("{} transfer-to for the 1st time!", to);
                        }
                        Some(prev_delta) => {
                            table[to as usize].delta =
                                Some(prev_delta.add(&u128_to_fr::<E>(amount)));
                            table[to as usize].income += amount;

                            #[cfg(feature = "std")]
                            println!("{} has presented and transfer-to again!", to);
                        }
                    }
                }
                TxType::Register(to) => {
                    // A user must be registered to got paid.
                    // So the Registration should happen on a new user.
                    table[to as usize] = Tmp {
                        delta: Some(tx.addr.mul(&mul160).add(&mul128)),
                        income: 0,
                        outcome: 0,
                        point_value: Some(tx.point_value()),
                        cur_nonce: tx.nonce,
                        balance: tx.balance,
                    };

                    #[cfg(feature = "std")]
                    println!("{} register for the 1st time!", to);
                }
            }
        }

        #[cfg(feature = "std")]
        println!("ending dark magic...(off-chain block.verify)");

        let mut point_values = Vec::new();
        let mut tmp_commit = self.commit.clone();

        for point in &points2prove {
            point_values.push(table[*point as usize].point_value.unwrap());
            tmp_commit = update_commit(
                &tmp_commit,
                table[*point as usize].delta.unwrap(),
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

        Ok((incomes, outcomes))
    }
}
