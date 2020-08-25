use async_std::task;
mod ckb;

fn main() {
    //task::block_on(ckb::query_block_deposits(1211));
    task::block_on(ckb::init_state(
        "0x365698b50ca0da75dca2c87f9e7b563811d3b5813736b8cc62cc3b106faceb17".to_owned(),
        "0x365698b50ca0da75dca2c87f9e7b563811d3b5813736b8cc62cc3b106faceb17".to_owned(),
        "0x23842d044d875bc30c9044dd5d97dff5b9483b42dcb001f1b35f221c323558d2".to_owned(),
    ));
}
