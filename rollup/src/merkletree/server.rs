use async_std::task;
//use tide::{Error, Request};

fn main() {
    tide::log::start();
    let mut app = tide::new();
    app.at("/")
        .get(|_| async { Ok("Merkle Tree Rollup is running!") });
    task::block_on(app.listen("127.0.0.1:8001")).unwrap();
}
