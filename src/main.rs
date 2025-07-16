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
    let (server_ready_tx, server_ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server_handle = tokio::spawn(async move {
        println!("Starting gRPC server on {}", server_addr);
        match server::start_server(server_addr).await {
            Ok(service) => {
                println!("gRPC server started successfully");
                // サーバーが起動したことを通知
                let _ = server_ready_tx.send(Ok(()));

                // シャットダウンシグナルを待機
                let _ = shutdown_rx.await;
                println!("Server received shutdown signal, creating snapshot...");

                // シャットダウン時にスナップショットを作成
                if let Err(e) = service.save_to_disk().await {
                    eprintln!("Failed to save snapshot during shutdown: {}", e);
                } else {
                    println!("Snapshot saved successfully during shutdown");
                }
            }
            Err(e) => {
                eprintln!("gRPC server error: {}", e);
                // エラーの場合はエラーを通知
                let _ = server_ready_tx.send(Err(e));
            }
        }
    });

    // サーバーが起動するまで待機
    println!("Waiting for gRPC server to start...");
    match server_ready_rx.await {
        Ok(Ok(())) => {
            println!("gRPC server startup notification received");
        }
        Ok(Err(e)) => {
            eprintln!("gRPC server failed to start: {}", e);
            return Err(format!("gRPC server startup failed: {}", e).into());
        }
        Err(_) => {
            eprintln!("Failed to receive server startup notification");
            return Err("Server startup notification channel closed".into());
        }
    }

    // RESTプロキシの起動
    println!("REST proxy address: {}", proxy_addr);
    let mut proxy_handle = tokio::spawn(async move {
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

    // プロキシまたはサーバーが何らかで終了するか、シャットダウンシグナルを受信するまで待機
    tokio::select! {
        _ = &mut server_handle => {
            println!("gRPC server stopped");
        }
        _ = &mut proxy_handle => {
            println!("REST proxy stopped");
        }
        _ = shutdown_signal => {
            println!("Shutdown signal received, initiating graceful shutdown...");

            // サーバーにシャットダウンシグナルを送信
            let _ = shutdown_tx.send(());

            // サーバーのスナップショット作成が完了するまで待機（タイムアウトあり）
            println!("Waiting for server to complete snapshot creation...");
            let timeout = tokio::time::timeout(
                tokio::time::Duration::from_secs(60),
                server_handle
            );

            match timeout.await {
                Ok(_) => {
                    println!("Graceful shutdown completed");
                }
                Err(_) => {
                    println!("Shutdown timeout exceeded, forcing termination");
                }
            }
        }
    }

    Ok(())
}
