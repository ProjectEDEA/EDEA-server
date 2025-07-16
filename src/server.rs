use prost::Message;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::fs;
use tokio::time::interval;
use tonic::{transport::Server, Request, Response, Status};
use tonic_web::GrpcWebLayer;
use tower_http::cors::CorsLayer;

pub mod class {
    tonic::include_proto!("class");
}

use class::{
    diagram_service_server::{DiagramService, DiagramServiceServer},
    File, FileId, Result as ProtoResult,
};

#[derive(Debug, Default)]
pub struct DiagramServiceImpl {
    // ファイルをメモリ内に保存するためのストレージ
    files: Arc<Mutex<HashMap<String, File>>>,
    // 永続化ディレクトリのパス
    persistence_dir: String,
}

impl DiagramServiceImpl {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            persistence_dir: "data".to_string(),
        }
    }

    // ファイルをディスクに保存
    pub async fn save_to_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let files = {
            let files_guard = self.files.lock().map_err(|_| "Failed to acquire lock")?;
            files_guard.clone() // Mutexからデータをクローンしてロックを解放
        };

        // ディレクトリが存在しない場合は作成
        fs::create_dir_all(&self.persistence_dir).await?;

        for (file_id, file) in files.iter() {
            let file_path = format!("{}/{}.bin", self.persistence_dir, file_id);

            // FileメッセージをProtobufバイナリにシリアライズ
            let mut buffer = Vec::new();
            file.encode(&mut buffer)?;

            fs::write(&file_path, buffer).await?;
        }

        println!("Saved {} files to disk", files.len());
        Ok(())
    }

    // ディスクからファイルを読み込み
    pub async fn load_from_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let persistence_dir = std::path::Path::new(&self.persistence_dir);

        if !persistence_dir.exists() {
            println!("Persistence directory does not exist, starting with empty storage");
            return Ok(());
        }

        let mut dir_entries = fs::read_dir(persistence_dir).await?;
        let mut loaded_count = 0;

        while let Some(entry) = dir_entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("bin") {
                let file_content = fs::read(&path).await?;

                // バイナリデータからFileメッセージを復元
                let file = File::decode(&file_content[..])?;

                if let Some(file_id) = &file.file_id {
                    let mut files = self.files.lock().map_err(|_| "Failed to acquire lock")?;

                    files.insert(file_id.id.clone(), file);
                    loaded_count += 1;
                }
            }
        }

        println!("Loaded {} files from disk", loaded_count);
        Ok(())
    }

    // 定期的な保存タスクを開始
    pub fn start_periodic_save(&self, interval_minutes: u64) {
        let files = Arc::clone(&self.files);
        let persistence_dir = self.persistence_dir.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_minutes * 60));

            loop {
                interval.tick().await;

                // DiagramServiceImplのインスタンスを作成してsave_to_diskを呼び出し
                let service = DiagramServiceImpl {
                    files: Arc::clone(&files),
                    persistence_dir: persistence_dir.clone(),
                };

                if let Err(e) = service.save_to_disk().await {
                    eprintln!("Failed to save files to disk: {}", e);
                } else {
                    println!("Periodic save completed successfully");
                }
            }
        });
    }
}

#[tonic::async_trait]
impl DiagramService for DiagramServiceImpl {
    async fn save_class_diagram(
        &self,
        request: Request<File>,
    ) -> Result<Response<ProtoResult>, Status> {
        let file = request.into_inner();

        // ファイルIDが存在するかチェック
        if let Some(file_id) = &file.file_id {
            let mut files = self
                .files
                .lock()
                .map_err(|_| Status::internal("Failed to acquire lock"))?;

            // ファイルを保存
            files.insert(file_id.id.clone(), file);

            let result = ProtoResult {
                value: true,
                message: Some("Class diagram saved successfully".to_string()),
            };

            Ok(Response::new(result))
        } else {
            let result = ProtoResult {
                value: false,
                message: Some("File ID is required".to_string()),
            };
            Ok(Response::new(result))
        }
    }

    async fn get_class_diagram(&self, request: Request<FileId>) -> Result<Response<File>, Status> {
        let file_id = request.into_inner();

        let files = self
            .files
            .lock()
            .map_err(|_| Status::internal("Failed to acquire lock"))?;

        if let Some(file) = files.get(&file_id.id) {
            Ok(Response::new(file.clone()))
        } else {
            Err(Status::not_found("File not found"))
        }
    }

    async fn is_existing_class_diagram(
        &self,
        request: Request<FileId>,
    ) -> Result<Response<ProtoResult>, Status> {
        let file_id = request.into_inner();

        let files = self
            .files
            .lock()
            .map_err(|_| Status::internal("Failed to acquire lock"))?;

        let exists = files.contains_key(&file_id.id);

        let result = ProtoResult {
            value: exists,
            message: if exists {
                Some("File exists".to_string())
            } else {
                Some("File does not exist".to_string())
            },
        };

        Ok(Response::new(result))
    }

    async fn delete_class_diagram(
        &self,
        request: Request<FileId>,
    ) -> Result<Response<ProtoResult>, Status> {
        let file_id = request.into_inner();

        let mut files = self
            .files
            .lock()
            .map_err(|_| Status::internal("Failed to acquire lock"))?;

        let removed = files.remove(&file_id.id).is_some();

        let result = ProtoResult {
            value: removed,
            message: if removed {
                Some("Class diagram deleted successfully".to_string())
            } else {
                Some("File not found".to_string())
            },
        };

        Ok(Response::new(result))
    }
}

pub async fn start_server(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let diagram_service = DiagramServiceImpl::new();

    // 起動時にディスクからファイルを読み込み
    if let Err(e) = diagram_service.load_from_disk().await {
        eprintln!("Warning: Failed to load files from disk: {}", e);
    }

    // n分間隔で定期的にファイルを保存
    diagram_service.start_periodic_save(1);

    println!("DiagramService gRPC server listening on {}", addr);

    // CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    Server::builder()
        .accept_http1(true)
        .layer(GrpcWebLayer::new())
        .layer(cors)
        .add_service(DiagramServiceServer::new(diagram_service))
        .serve(addr)
        .await?;

    Ok(())
}
