mod server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting EDEA gRPC server...");
    
    server::start_server().await?;
    
    Ok(())
}