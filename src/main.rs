use std::net::SocketAddr;
mod proxy;
mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting EDEA gRPC server...");

    let server_addr: SocketAddr = "localhost:50051".parse()?;
    server::start_server(server_addr.clone()).await?;

    let proxy_addr: SocketAddr = "localhost:3000".parse()?;
    proxy::start_proxy(proxy_addr, server_addr).await?;

    Ok(())
}
