use ckb_zkp::gadgets::mimc;
use ckb_zkp::math::{io, serialize::*, BigInteger, FromBytes, PairingEngine, PrimeField, ToBytes};

use ckb_zkp::scheme::asvc::{Proof, UpdateKey};

pub const ACCOUNT_SIZE: usize = 16;

pub type TxHash = Vec<u8>;

#[derive(Clone)]
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
    pub fn addr(&self) -> E::Fr {
        let mut bytes = vec![];
        self.i.write(&mut bytes).unwrap();
        self.update_key.ai.write(&mut bytes).unwrap();
        self.update_key.ui.write(&mut bytes).unwrap();
        self.tradition_pubkey.0.write(&mut bytes).unwrap();

        mimc::hash(&bytes)
    }
}

#[derive(Clone)]
pub enum TxType<E: PairingEngine> {
    /// to_account, amount.
    Deposit(u32, u128),
    /// from_account, amount.
    Withdraw(u32, u128),
    /// from_account, to_account, amount, to's update_key
    Transfer(u32, u32, u128, UpdateKey<E>),
    /// registe a account.
    Register(u32, PublicKey),
}

impl<E: PairingEngine> TxType<E> {
    pub fn to_bytes(&self) -> Vec<u8> {
        // match self {
        //     TxType::Deposit(..) => 1u8,
        //     TxType::Withdraw(..) => 2u8,
        //     TxType::Transfer(..) => 3u8,
        //     TxType::Register(..) => 4u8,
        // }
        todo!()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ()> {
        // match u {
        //     1u8 => Ok(TxType::Deposit),
        //     2u8 => Ok(TxType::Withdraw),
        //     3u8 => Ok(TxType::Transfer),
        //     _ => Err(()),
        // }
        todo!()
    }

    pub fn new_transfer(from: u32, to: u32, amount: u128, to_upk: UpdateKey<E>) -> Self {
        TxType::Transfer(from, to, amount, to_upk)
    }

    pub fn new_deposit(to: u32, amount: u128) -> Self {
        TxType::Deposit(to, amount)
    }

    pub fn new_withdraw(from: u32, amount: u128) -> Self {
        TxType::Withdraw(from, amount)
    }

    pub fn new_register(account: u32, pubkey: PublicKey) -> Self {
        TxType::Register(account, pubkey)
    }
}

#[derive(Clone)]
pub struct Transaction<E: PairingEngine> {
    /// transaction type. include
    pub tx_type: TxType<E>,
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
        tx_type: TxType<E>,
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

    pub fn proof_param(&self) -> E::Fr {
        let mut bytes = Vec::new();
        self.addr.write(&mut bytes).unwrap();
        self.nonce.write(&mut bytes).unwrap();
        self.balance.to_le_bytes().write(&mut bytes).unwrap();

        mimc::hash(&bytes)
    }

    pub fn hash(&self) -> TxHash {
        vec![]
    }

    pub fn id(&self) -> String {
        "0x000000".to_owned()
    }

    /// new transfer transaction.
    pub fn new_transfer(
        from: u32,
        to: u32,
        amount: u128,
        to_upk: UpdateKey<E>,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_transfer(from, to, amount, to_upk);
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
        pubkey: PublicKey,
        fpk: FullPubKey<E>,
        nonce: u32,
        balance: u128,
        proof: Proof<E>,
        sk: &SecretKey,
    ) -> Self {
        let tx_type = TxType::new_register(account, pubkey);
        Self::new(tx_type, fpk, nonce, balance, proof, sk)
    }

    /// verify sign
    pub fn verify(&self) -> bool {
        true
    }

    pub fn sign(&mut self, sk: &SecretKey) {
        // TODO
    }
}

pub fn u128_to_fr<E: PairingEngine>(u: u128) -> E::Fr {
    E::Fr::from_repr(<E::Fr as PrimeField>::BigInt::from_u128(u))
}
