use ckb_zkp::{
    gadgets::mimc,
    math::{
        io::Result as IoResult, serialize::*, BigInteger, Field, FromBytes, PairingEngine,
        PrimeField, ToBytes, Zero,
    },
    scheme::asvc::{Proof, UpdateKey},
};
use core::ops::Neg;
use sha2::{Digest, Sha256};

use crate::{String, Vec};

pub const ACCOUNT_SIZE: usize = 2;

pub type TxHash = Vec<u8>;

#[derive(Clone, Eq, PartialEq)]
pub struct PublicKey(pub Vec<u8>);

pub struct SecretKey(pub Vec<u8>);

impl PublicKey {
    pub fn from_hex(s: &str) -> Result<Self, ()> {
        hex::decode(s).map(|v| PublicKey(v)).map_err(|_| ())
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }
}

impl SecretKey {
    pub fn from_hex(s: &str) -> Result<Self, ()> {
        hex::decode(s).map(|v| SecretKey(v)).map_err(|_| ())
    }

    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }
}

#[derive(Clone)]
pub struct FullPubKey<E: PairingEngine> {
    /// user_account_number.
    pub i: u32,
    /// user update proof's key.
    pub update_key: UpdateKey<E>,
    /// rollup defined kepair.
    pub tradition_pubkey: PublicKey,
}

impl<E: PairingEngine> FullPubKey<E> {
    pub fn default(i: u32, upk: UpdateKey<E>) -> Self {
        Self {
            i,
            update_key: upk,
            tradition_pubkey: PublicKey(Vec::new()),
        }
    }

    pub fn addr(&self) -> E::Fr {
        let mut bytes = Vec::new();
        self.i.write(&mut bytes).unwrap();
        self.update_key.ai.write(&mut bytes).unwrap();
        self.update_key.ui.write(&mut bytes).unwrap();
        self.tradition_pubkey.0.write(&mut bytes).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(bytes);
        mimc::hash(&hasher.finalize()[..])
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum TxType {
    /// to_account, amount.
    Deposit(u32, u128),
    /// from_account, amount.
    Withdraw(u32, u128),
    /// from_account, to_account, amount
    Transfer(u32, u32, u128),
    /// registe a account.
    Register(u32),
}

impl TxType {
    pub fn new_transfer(from: u32, to: u32, amount: u128) -> Self {
        TxType::Transfer(from, to, amount)
    }

    pub fn new_deposit(to: u32, amount: u128) -> Self {
        TxType::Deposit(to, amount)
    }

    pub fn new_withdraw(from: u32, amount: u128) -> Self {
        TxType::Withdraw(from, amount)
    }

    pub fn new_register(account: u32) -> Self {
        TxType::Register(account)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Transaction<E: PairingEngine> {
    /// transaction type. include
    pub tx_type: TxType,
    /// ownership proof.
    pub proof: Proof<E>,
    /// account's hash.
    pub addr: E::Fr,
    /// tx's nonce
    pub nonce: u32,
    /// from_account's balance.
    pub balance: u128,
    /// sender's pubkey.
    pub pubkey: PublicKey,
    /// sender's sign.
    pub sign: Vec<u8>,
}

impl<E: PairingEngine> Default for Transaction<E> {
    fn default() -> Self {
        todo!()
    }
}

impl<E: PairingEngine> Transaction<E> {
    fn new(
        tx_type: TxType,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let mut tx = Self {
            tx_type,
            proof,
            nonce,
            balance,
            addr: fpk.addr(),
            pubkey: fpk.tradition_pubkey,
            sign: Vec::new(),
        };
        tx.sign(sk);

        tx
    }

    pub fn hash(&self) -> TxHash {
        let mut bytes = Vec::new();

        // mock
        self.from().write(&mut bytes).unwrap();
        self.addr.write(&mut bytes).unwrap();
        self.nonce.write(&mut bytes).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher.finalize().to_vec()
    }

    pub fn from(&self) -> u32 {
        match self.tx_type {
            TxType::Deposit(from, ..)
            | TxType::Withdraw(from, ..)
            | TxType::Transfer(from, ..)
            | TxType::Register(from, ..) => from,
        }
    }

    #[rustfmt::skip]
    pub fn point_value(&self) -> E::Fr {
        let mul_160: E::Fr = E::Fr::from(2).pow(&[160]);
        let mul_128: E::Fr = E::Fr::from(2).pow(&[128]);

        match self.tx_type {
            TxType::Deposit(..) | TxType::Withdraw(..) => {
                self.addr * &mul_160
                    + &(mul_128 * &u32_to_fr::<E>(self.nonce))
                    + &u128_to_fr::<E>(self.balance)
            }
            TxType::Transfer(..) => {
                self.addr * &mul_160
                    + &(mul_128 * &u32_to_fr::<E>(self.nonce - 1))
                    + &u128_to_fr::<E>(self.balance)
            }
            TxType::Register(..) => {
                E::Fr::zero()
                    + &E::Fr::zero()
                    + &E::Fr::zero()
            }
        }
    }

    #[rustfmt::skip]
    pub fn delta_value(&self) -> (E::Fr, E::Fr) {
        let mul_160: E::Fr = E::Fr::from(2).pow(&[160]);
        let mul_128: E::Fr = E::Fr::from(2).pow(&[128]);
        let zero = E::Fr::zero();

        match self.tx_type {
            TxType::Deposit(_from, amount) => {
                (u128_to_fr::<E>(amount), zero)
            }
            TxType::Withdraw(_from, amount) => {
                (u128_to_fr::<E>(amount).neg(), zero)
            }
            TxType::Transfer(_from, _to, amount) => {
                let amount_fr = u128_to_fr::<E>(amount);
                (amount_fr.neg() + &mul_160,
                 amount_fr)
            }
            TxType::Register(..) => {
                (self.addr * &mul_160, zero)
            }
        }
    }

    pub fn id(&self) -> String {
        hex::encode(self.hash())
    }

    /// new transfer transaction.
    pub fn new_transfer(
        from: u32,
        to: u32,
        amount: u128,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_transfer(from, to, amount);
        Self::new(tx_type, fpk, nonce, balance, proof, sk)
    }

    pub fn new_deposit(
        to: u32,
        amount: u128,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_deposit(to, amount);
        Self::new(tx_type, fpk, nonce, balance, proof, sk)
    }

    pub fn new_withdraw(
        from: u32,
        amount: u128,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_withdraw(from, amount);
        Self::new(tx_type, fpk, nonce, balance, proof, sk)
    }

    pub fn new_register(
        account: u32,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_register(account);
        Self::new(tx_type, fpk, nonce, balance, proof, sk)
    }

    /// verify sign
    pub fn verify(&self) -> bool {
        true
    }

    pub fn sign(&mut self, _sk: &SecretKey) {
        // TODO
    }
}

pub fn u128_to_fr<E: PairingEngine>(u: u128) -> E::Fr {
    E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(u))
}

pub fn u32_to_fr<E: PairingEngine>(u: u32) -> E::Fr {
    E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from(u as u64))
}

impl ToBytes for TxType {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        match self {
            TxType::Deposit(from, amount) => {
                0u8.write(&mut writer)?;
                from.write(&mut writer)?;
                amount.write(&mut writer)?;
            }
            TxType::Withdraw(from, amount) => {
                1u8.write(&mut writer)?;
                from.write(&mut writer)?;
                amount.write(&mut writer)?;
            }
            TxType::Register(from) => {
                2u8.write(&mut writer)?;
                from.write(&mut writer)?;
            }
            TxType::Transfer(from, to, amount) => {
                3u8.write(&mut writer)?;
                from.write(&mut writer)?;
                to.write(&mut writer)?;
                amount.write(&mut writer)?;
            }
        }

        Ok(())
    }
}

impl FromBytes for TxType {
    #[inline]
    fn read<R: Read>(mut reader: R) -> IoResult<Self> {
        let tx_type = u8::read(&mut reader)?;

        match tx_type {
            0u8 => {
                let from = u32::read(&mut reader)?;
                let amount = u128::read(&mut reader)?;

                Ok(TxType::Deposit(from, amount))
            }
            1u8 => {
                let from = u32::read(&mut reader)?;
                let amount = u128::read(&mut reader)?;

                Ok(TxType::Withdraw(from, amount))
            }
            2u8 => {
                let from = u32::read(&mut reader)?;

                Ok(TxType::Register(from))
            }
            3u8 => {
                let from = u32::read(&mut reader)?;
                let to = u32::read(&mut reader)?;
                let amount = u128::read(&mut reader)?;

                Ok(TxType::Transfer(from, to, amount))
            }
            _ => panic!("Invalid tx"),
        }
    }
}

impl ToBytes for PublicKey {
    #[inline]
    fn write<W: Write>(&self, _writer: W) -> IoResult<()> {
        Ok(())
    }
}

impl FromBytes for PublicKey {
    #[inline]
    fn read<R: Read>(_reader: R) -> IoResult<Self> {
        Ok(Self(Vec::new()))
    }
}

impl<E: PairingEngine> ToBytes for Transaction<E> {
    #[inline]
    fn write<W: Write>(&self, mut writer: W) -> IoResult<()> {
        self.tx_type.write(&mut writer)?;
        self.proof.write(&mut writer)?;
        self.addr.write(&mut writer)?;
        self.nonce.write(&mut writer)?;
        self.balance.write(&mut writer)?;
        self.pubkey.write(&mut writer)

        //self.sign.write(&mut writer)?
    }
}

impl<E: PairingEngine> FromBytes for Transaction<E> {
    #[inline]
    fn read<R: Read>(mut reader: R) -> IoResult<Self> {
        let tx_type = TxType::read(&mut reader)?;
        let proof = Proof::read(&mut reader)?;
        let addr = E::Fr::read(&mut reader)?;
        let nonce = u32::read(&mut reader)?;
        let balance = u128::read(&mut reader)?;
        let pubkey = PublicKey::read(&mut reader)?;

        Ok(Self {
            tx_type,
            proof,
            addr,
            nonce,
            balance,
            pubkey,
            sign: Vec::new(),
        })
    }
}
