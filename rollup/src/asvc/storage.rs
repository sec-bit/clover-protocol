use std::collections::HashMap;

use crate::block::Block;
use crate::transaction::{Transaction, TxHash};

pub struct Storage {
    block_height: u32,
    blocks: Vec<Block>,
    pools: HashMap<TxHash, Transaction>,
}

impl Storage {
    pub fn init() -> Self {
        Self {
            block_height: 0,
            blocks: vec![],
            pools: HashMap::new(),
        }
    }

    pub fn try_insert_tx(&mut self, tx: Transaction) -> bool {
        let tx_hash = tx.hash();

        if !self.pools.contains_key(&tx_hash) {
            self.pools.insert(tx_hash, tx);
        }

        true
    }

    pub fn create_block(&mut self) -> Option<Block> {
        None
    }
}
