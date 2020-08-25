use async_std::task;
use tide::{Error, Request};

use rollup::asvc::block::Block;

async fn deposit(mut req: Request<()>) -> Result<String, Error> {
    let data = req.body_string().await?;
    println!("Recv deposit hex: {:?}, len: {}", data, data.len());

    Ok("0x".to_owned())
}

async fn withdraw(mut req: Request<()>) -> Result<String, Error> {
    let data = req.body_string().await?;
    println!("Recv withdraw hex: {:?}, len: {}", data, data.len());

    Ok("0x".to_owned())
}

async fn block(mut req: Request<()>) -> Result<String, Error> {
    let data = req.body_string().await?;
    println!("Recv Block hex: {:?}, len: {}", data, data.len());

    if let Ok(_block) = Block::from_hex(&data) {
        // TODO verify block
    }

    Ok("0x".to_owned())
}

fn main() {
    tide::log::start();
    let mut app = tide::new();

    // contracts
    app.at("/deposit").post(deposit);
    app.at("/withdraw").post(withdraw);
    app.at("/block").post(block);

    // node
    app.at("/").get(|_| async { Ok("Hello, world!") });

    task::block_on(app.listen("127.0.0.1:8000")).unwrap();
}