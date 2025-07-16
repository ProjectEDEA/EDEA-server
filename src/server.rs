use prost::Message;
use std::collections::HashMap;
use std::io::Read;
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

#[derive(Debug, Default, Clone)]
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

    // インメモリ情報をディスクにダンプ
    pub async fn save_to_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let files = {
            let files_guard = self.files.lock().map_err(|_| "Failed to acquire lock")?;
            files_guard.clone() // Mutexからデータをクローンしてロックを解放
        };

        // ディレクトリが存在しない場合は作成
        tokio::fs::create_dir_all(&self.persistence_dir).await?;
        let file_path = format!("{}/snapshot.bin", self.persistence_dir);
        
        // HashMapをバイナリとしてシリアライズ
        let mut buffer = Vec::new();
        
        // ファイル数を最初に書き込み
        let file_count = files.len() as u32;
        buffer.extend_from_slice(&file_count.to_be_bytes());
        
        // 各ファイルをエンコード
        for (file_id, file) in files.iter() {
            // ファイルIDの長さとファイルIDを書き込み
            let file_id_bytes = file_id.as_bytes();
            let file_id_len = file_id_bytes.len() as u32;
            buffer.extend_from_slice(&file_id_len.to_be_bytes());
            buffer.extend_from_slice(file_id_bytes);
            
            // ファイルデータをエンコード
            let mut file_buffer = Vec::new();
            file.encode(&mut file_buffer)?;
            
            // ファイルデータの長さとファイルデータを書き込み
            let file_data_len = file_buffer.len() as u32;
            buffer.extend_from_slice(&file_data_len.to_be_bytes());
            buffer.extend_from_slice(&file_buffer);
        }
        
        tokio::fs::write(file_path, buffer).await?;

        println!("Saved {} files snapshot to disk", files.len());
        Ok(())
    }

    // ファイルをディスクにエクスポート
    pub async fn export_files(&self) -> Result<(), Box<dyn std::error::Error>> {
        let files = self.files.lock().map_err(|_| "Failed to acquire lock")?;
        let date = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();

        // エクスポートディレクトリを作成
        tokio::fs::create_dir_all(format!("{}/exported/{}", self.persistence_dir, date)).await?;
        for (file_id, file) in files.iter() {
            let file_path = format!("{}/exported/{}/{}.bin", self.persistence_dir, date, file_id);

            // FileメッセージをProtobufバイナリにシリアライズ
            let mut buffer = Vec::new();
            file.encode(&mut buffer)?;

            tokio::fs::write(&file_path, buffer).await?;
        }

        Ok(())
    }

    // ディスクからファイルを読み込み
    pub async fn load_from_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let persistence_dir = std::path::Path::new(&self.persistence_dir);

        if !persistence_dir.exists() {
            println!("Persistence directory does not exist, starting with empty storage");
            return Ok(());
        }

        let snapshot_path = format!("{}/snapshot.bin", self.persistence_dir);
        let snapshot_file = std::path::Path::new(&snapshot_path);

        if !snapshot_file.exists() {
            println!("Snapshot file does not exist, starting with empty storage");
            return Ok(());
        }

        let file_content = fs::read(snapshot_file).await?;
        let mut cursor = std::io::Cursor::new(file_content);
        
        // ファイル数を読み取り
        let mut file_count_bytes = [0u8; 4];
        cursor.read_exact(&mut file_count_bytes)?;
        let file_count = u32::from_be_bytes(file_count_bytes);
        
        let mut files = self.files.lock().map_err(|_| "Failed to acquire lock")?;
        
        // 各ファイルをデコード
        for _ in 0..file_count {
            // ファイルIDの長さを読み取り
            let mut file_id_len_bytes = [0u8; 4];
            cursor.read_exact(&mut file_id_len_bytes)?;
            let file_id_len = u32::from_be_bytes(file_id_len_bytes) as usize;
            
            // ファイルIDを読み取り
            let mut file_id_bytes = vec![0u8; file_id_len];
            cursor.read_exact(&mut file_id_bytes)?;
            let file_id = String::from_utf8(file_id_bytes)?;
            
            // ファイルデータの長さを読み取り
            let mut file_data_len_bytes = [0u8; 4];
            cursor.read_exact(&mut file_data_len_bytes)?;
            let file_data_len = u32::from_be_bytes(file_data_len_bytes) as usize;
            
            // ファイルデータを読み取り
            let mut file_data_bytes = vec![0u8; file_data_len];
            cursor.read_exact(&mut file_data_bytes)?;
            
            // ファイルデータをデコード
            let file = File::decode(&file_data_bytes[..])?;
            files.insert(file_id, file);
        }

        println!("Loaded {} files from disk", files.len());
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

pub async fn start_server(addr: SocketAddr) -> Result<Arc<DiagramServiceImpl>, String> {
    let diagram_service = Arc::new(DiagramServiceImpl::new());

    // 起動時にディスクからファイルを読み込み
    if let Err(e) = diagram_service.load_from_disk().await {
        return Err(format!("Failed to load files from disk: {}", e));
    }

    // n分間隔で定期的にファイルを保存
    diagram_service.start_periodic_save(1);

    println!("DiagramService gRPC server listening on {}", addr);

    // CORS
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let service_clone = Arc::clone(&diagram_service);
    
    // サーバーをバックグラウンドで起動
    tokio::spawn(async move {
        println!("gRPC server starting...");
        if let Err(e) = Server::builder()
            .accept_http1(true)
            .layer(GrpcWebLayer::new())
            .layer(cors)
            .add_service(DiagramServiceServer::new((*service_clone).clone()))
            .serve(addr)
            .await
        {
            eprintln!("gRPC server error: {}", e);
        }
    });

    // サーバーが起動するまで少し待機
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    println!("gRPC server should be ready now");

    Ok(diagram_service)
}
