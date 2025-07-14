use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tonic::{transport::Server, Request, Response, Status};

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
}

impl DiagramServiceImpl {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
        }
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
            let mut files = self.files.lock().map_err(|_| {
                Status::internal("Failed to acquire lock")
            })?;
            
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

    async fn get_class_diagram(
        &self,
        request: Request<FileId>,
    ) -> Result<Response<File>, Status> {
        let file_id = request.into_inner();
        
        let files = self.files.lock().map_err(|_| {
            Status::internal("Failed to acquire lock")
        })?;
        
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
        
        let files = self.files.lock().map_err(|_| {
            Status::internal("Failed to acquire lock")
        })?;
        
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
        
        let mut files = self.files.lock().map_err(|_| {
            Status::internal("Failed to acquire lock")
        })?;
        
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

pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:50051".parse()?;
    let diagram_service = DiagramServiceImpl::new();

    println!("DiagramService gRPC server listening on {}", addr);

    Server::builder()
        .add_service(DiagramServiceServer::new(diagram_service))
        .serve(addr)
        .await?;

    Ok(())
}