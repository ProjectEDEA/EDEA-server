use std::net::SocketAddr;
use tokio::signal;
mod proxy;
mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting EDEA gRPC server and REST proxy...");

    let server_addr: SocketAddr = "127.0.0.1:50051"
        .parse()
        .map_err(|e| format!("Failed to parse server address: {}", e))?;
    let proxy_addr: SocketAddr = "127.0.0.1:3000"
        .parse()
        .map_err(|e| format!("Failed to parse proxy address: {}", e))?;

    // gRPCサーバの起動
    println!("gRPC server address: {}", server_addr);
    let server_handle = tokio::spawn(async move {
        println!("Starting gRPC server on {}", server_addr);
        if let Err(e) = server::start_server(server_addr).await {
            eprintln!("gRPC server error: {}", e);
        }
    });

    // RESTプロキシの起動
    println!("REST proxy address: {}", proxy_addr);
    let proxy_handle = tokio::spawn(async move {
        println!("Starting REST proxy on {}", proxy_addr);
        if let Err(e) = proxy::start_proxy(proxy_addr, server_addr).await {
            eprintln!("REST proxy error: {}", e);
        }
    });

    // シャットダウンシグナルを待機
    let shutdown_signal = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        println!("Received shutdown signal, stopping servers...");
    };

    // どちらかのタスクが完了するか、シャットダウンシグナルを受信するまで待機
    tokio::select! {
        _ = server_handle => {
            println!("gRPC server stopped");
        }
        _ = proxy_handle => {
            println!("REST proxy stopped");
        }
        _ = shutdown_signal => {
            println!("Shutdown signal received, terminating...");
        }
    }

    Ok(())
}
