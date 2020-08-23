use async_std::task;
mod ckb;

fn main() {
    //task::block_on(ckb::query_block_deposits(1211));
    task::block_on(ckb::post_block());
}
