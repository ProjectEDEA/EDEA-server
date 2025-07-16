use axum::{
    extract::{Path, State},
    response::Json,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;

pub mod class {
    tonic::include_proto!("class");
}

use class::{
    diagram_service_client::DiagramServiceClient, Class, File, FileId, Method, Multiplicity,
    Relation, RelationInfo, RelationInfoList, Variable, Visibility,
};

pub async fn start_proxy(
    proxy_addr: SocketAddr,
    dest_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(proxy_addr).await?;
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/api_p1", post(save_diagram))
        .route("/api_p1/{file_id}", get(get_diagram))
        .route("/api_p1/{file_id}", delete(delete_diagram))
        .route("/api_p1/{file_id}/exists", get(check_exists))
        .layer(cors)
        .with_state(dest_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn save_diagram(
    State(dest_addr): State<SocketAddr>,
    Json(json): Json<serde_json::Value>,
) -> Result<String, String> {
    // Logic to save the diagram
    println!("Saving diagram: {:?}", json);

    // gRPCクライアントを作成し、データを転送
    let mut client = DiagramServiceClient::connect(format!("http://{}", dest_addr))
        .await
        .map_err(|e| format!("Failed to connect to gRPC server: {}", e))?;

    // JSONをprotoのFile構造体に変換
    let file = json_to_proto_file(json)?;

    // gRPCリクエストを作成
    let request = tonic::Request::new(file);

    // gRPCサーバに送信
    let response = client
        .save_class_diagram(request)
        .await
        .map_err(|e| format!("Failed to save diagram: {}", e))?;

    let result = response.into_inner();
    if result.value {
        Ok("Diagram saved successfully".to_string())
    } else {
        Err(result
            .message
            .unwrap_or_else(|| "Unknown error".to_string()))
    }
}

async fn get_diagram(
    State(dest_addr): State<SocketAddr>,
    Path(file_id): Path<String>,
) -> Result<Json<serde_json::Value>, String> {
    // Logic to retrieve the diagram
    println!("Retrieving diagram for file_id: {}", file_id);

    // gRPCクライアントを作成
    let mut client = DiagramServiceClient::connect(format!("http://{}", dest_addr))
        .await
        .map_err(|e| format!("Failed to connect to gRPC server: {}", e))?;

    // gRPCリクエストを作成
    let request = tonic::Request::new(FileId {
        id: file_id.clone(),
    });

    // gRPCサーバから取得
    let response = client
        .get_class_diagram(request)
        .await
        .map_err(|e| format!("Failed to get diagram: {}", e))?;

    let file = response.into_inner();

    // protoのFileをJSONに変換
    let json = proto_file_to_json(&file);

    Ok(Json(json))
}

async fn delete_diagram(
    State(dest_addr): State<SocketAddr>,
    Path(file_id): Path<String>,
) -> Result<String, String> {
    // Logic to delete the diagram
    println!("Deleting diagram for file_id: {}", file_id);

    // gRPCクライアントを作成
    let mut client = DiagramServiceClient::connect(format!("http://{}", dest_addr))
        .await
        .map_err(|e| format!("Failed to connect to gRPC server: {}", e))?;

    // gRPCリクエストを作成
    let request = tonic::Request::new(FileId {
        id: file_id.clone(),
    });

    // サーバから削除
    let response = client
        .delete_class_diagram(request)
        .await
        .map_err(|e| format!("Failed to delete diagram: {}", e))?;

    let result = response.into_inner();
    if result.value {
        Ok("Diagram deleted successfully".to_string())
    } else {
        Err(result
            .message
            .unwrap_or_else(|| "Unknown error".to_string()))
    }
}

async fn check_exists(
    State(dest_addr): State<SocketAddr>,
    Path(file_id): Path<String>,
) -> Result<Json<serde_json::Value>, String> {
    // Logic to check if diagram exists
    println!("Checking existence of diagram for file_id: {}", file_id);

    // gRPCクライアントを作成
    let mut client = DiagramServiceClient::connect(format!("http://{}", dest_addr))
        .await
        .map_err(|e| format!("Failed to connect to gRPC server: {}", e))?;

    // gRPCリクエストを作成
    let request = tonic::Request::new(FileId {
        id: file_id.clone(),
    });

    // gRPCサーバから確認
    let response = client
        .is_existing_class_diagram(request)
        .await
        .map_err(|e| format!("Failed to check diagram existence: {}", e))?;

    let result = response.into_inner();

    Ok(Json(serde_json::json!({
        "exists": result.value,
        "message": result.message
    })))
}

// JSONをprotoのFile構造体に変換する関数
fn json_to_proto_file(json: serde_json::Value) -> Result<File, String> {
    let file_id = json
        .get("file_id")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let last_modified = json
        .get("last_modified")
        .and_then(|v| v.as_i64())
        .unwrap_or_default() as i32;

    let created_at = json
        .get("created_at")
        .and_then(|v| v.as_i64())
        .unwrap_or_default() as i32;

    let classes = json
        .get("classes")
        .and_then(|v| v.as_array())
        .map(|classes| {
            classes
                .iter()
                .filter_map(|class| json_to_proto_class(class).ok())
                .collect()
        })
        .unwrap_or_default();

    Ok(File {
        last_modified,
        created_at,
        file_id: Some(FileId {
            id: file_id.to_string(),
        }),
        name: name.to_string(),
        classes,
    })
}

fn json_to_proto_class(json: &serde_json::Value) -> Result<Class, String> {
    let id = json.get("id").and_then(|v| v.as_str()).unwrap_or_default();

    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let attributes = json
        .get("attributes")
        .and_then(|v| v.as_array())
        .map(|attrs| {
            attrs
                .iter()
                .filter_map(|attr| json_to_proto_variable(attr).ok())
                .collect()
        })
        .unwrap_or_default();

    let methods = json
        .get("methods")
        .and_then(|v| v.as_array())
        .map(|methods| {
            methods
                .iter()
                .filter_map(|method| json_to_proto_method(method).ok())
                .collect()
        })
        .unwrap_or_default();

    let relations = json
        .get("relations")
        .and_then(|v| v.get("relation_infos"))
        .and_then(|v| v.as_array())
        .map(|relations| {
            let relation_infos = relations
                .iter()
                .filter_map(|rel| json_to_proto_relation(rel).ok())
                .collect();
            RelationInfoList { relation_infos }
        });

    Ok(Class {
        id: id.to_string(),
        name: name.to_string(),
        relations,
        attributes,
        methods,
    })
}

fn json_to_proto_variable(json: &serde_json::Value) -> Result<Variable, String> {
    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let r#type = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let visibility = json
        .get("visibility")
        .and_then(|v| v.as_str())
        .and_then(|v| match v {
            "PUBLIC" => Some(Visibility::Public as i32),
            "PRIVATE" => Some(Visibility::Private as i32),
            "PROTECTED" => Some(Visibility::Protected as i32),
            _ => Some(Visibility::NonModifier as i32),
        });

    let is_static = json.get("is_static").and_then(|v| v.as_bool());

    Ok(Variable {
        name: name.to_string(),
        r#type: r#type.to_string(),
        visibility,
        is_static,
    })
}

fn json_to_proto_method(json: &serde_json::Value) -> Result<Method, String> {
    let name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let return_type = json
        .get("return_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let visibility = json
        .get("visibility")
        .and_then(|v| v.as_str())
        .and_then(|v| match v {
            "PUBLIC" => Some(Visibility::Public as i32),
            "PRIVATE" => Some(Visibility::Private as i32),
            "PROTECTED" => Some(Visibility::Protected as i32),
            _ => Some(Visibility::NonModifier as i32),
        })
        .unwrap_or(Visibility::NonModifier as i32);

    let is_abstract = json.get("is_abstract").and_then(|v| v.as_bool());

    let is_static = json.get("is_static").and_then(|v| v.as_bool());

    let parameters = json
        .get("parameters")
        .and_then(|v| v.as_array())
        .map(|params| {
            params
                .iter()
                .filter_map(|param| json_to_proto_variable(param).ok())
                .collect()
        })
        .unwrap_or_default();

    Ok(Method {
        name: name.to_string(),
        return_type: return_type.to_string(),
        visibility,
        is_abstract,
        is_static,
        parameters,
    })
}

fn json_to_proto_relation(json: &serde_json::Value) -> Result<RelationInfo, String> {
    let target_class_id = json
        .get("target_class_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let relation = json
        .get("relation")
        .and_then(|v| v.as_str())
        .and_then(|v| match v {
            "INHERITANCE" => Some(Relation::Inheritance as i32),
            "IMPLEMENTATION" => Some(Relation::Implementation as i32),
            "ASSOCIATION" => Some(Relation::Association as i32),
            "AGGREGATION" => Some(Relation::Aggregation as i32),
            "COMPOSITION" => Some(Relation::Composition as i32),
            _ => Some(Relation::None as i32),
        })
        .unwrap_or(Relation::None as i32);

    let multiplicity_p = json
        .get("multiplicity_p")
        .and_then(|v| json_to_proto_multiplicity(v).ok());

    let multiplicity_c = json
        .get("multiplicity_c")
        .and_then(|v| json_to_proto_multiplicity(v).ok());

    let role_name_p = json
        .get("role_name_p")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let role_name_c = json
        .get("role_name_c")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(RelationInfo {
        target_class_id: target_class_id.to_string(),
        relation,
        multiplicity_p,
        multiplicity_c,
        role_name_p,
        role_name_c,
    })
}

fn json_to_proto_multiplicity(json: &serde_json::Value) -> Result<Multiplicity, String> {
    let lower = json
        .get("lower")
        .and_then(|v| v.as_u64())
        .unwrap_or_default() as u32;

    let upper = json.get("upper").and_then(|v| v.as_u64()).map(|v| v as u32);

    Ok(Multiplicity { lower, upper })
}

// protoのFileをJSONに変換する関数
fn proto_file_to_json(file: &File) -> serde_json::Value {
    let file_id = file
        .file_id
        .as_ref()
        .map(|id| serde_json::json!({"id": id.id}))
        .unwrap_or_else(|| serde_json::json!({"id": ""}));

    let classes: Vec<serde_json::Value> = file.classes.iter().map(proto_class_to_json).collect();

    serde_json::json!({
        "last_modified": file.last_modified,
        "created_at": file.created_at,
        "file_id": file_id,
        "name": file.name,
        "classes": classes
    })
}

fn proto_class_to_json(class: &Class) -> serde_json::Value {
    let attributes: Vec<serde_json::Value> = class
        .attributes
        .iter()
        .map(proto_variable_to_json)
        .collect();

    let methods: Vec<serde_json::Value> = class.methods.iter().map(proto_method_to_json).collect();

    let relations = class
        .relations
        .as_ref()
        .map(|rel| {
            let relation_infos: Vec<serde_json::Value> = rel
                .relation_infos
                .iter()
                .map(proto_relation_to_json)
                .collect();
            serde_json::json!({"relation_infos": relation_infos})
        })
        .unwrap_or_else(|| serde_json::json!(null));

    serde_json::json!({
        "id": class.id,
        "name": class.name,
        "attributes": attributes,
        "methods": methods,
        "relations": relations
    })
}

fn proto_variable_to_json(variable: &Variable) -> serde_json::Value {
    let visibility = variable
        .visibility
        .and_then(|v| match v {
            v if v == Visibility::Public as i32 => Some("PUBLIC"),
            v if v == Visibility::Private as i32 => Some("PRIVATE"),
            v if v == Visibility::Protected as i32 => Some("PROTECTED"),
            _ => Some("NON_MODIFIER"),
        })
        .unwrap_or("NON_MODIFIER");

    serde_json::json!({
        "name": variable.name,
        "type": variable.r#type,
        "visibility": visibility,
        "is_static": variable.is_static
    })
}

fn proto_method_to_json(method: &Method) -> serde_json::Value {
    let visibility = match method.visibility {
        v if v == Visibility::Public as i32 => "PUBLIC",
        v if v == Visibility::Private as i32 => "PRIVATE",
        v if v == Visibility::Protected as i32 => "PROTECTED",
        _ => "NON_MODIFIER",
    };

    let parameters: Vec<serde_json::Value> = method
        .parameters
        .iter()
        .map(proto_variable_to_json)
        .collect();

    serde_json::json!({
        "name": method.name,
        "return_type": method.return_type,
        "visibility": visibility,
        "is_abstract": method.is_abstract,
        "is_static": method.is_static,
        "parameters": parameters
    })
}

fn proto_relation_to_json(relation: &RelationInfo) -> serde_json::Value {
    let relation_type = match relation.relation {
        v if v == Relation::Inheritance as i32 => "INHERITANCE",
        v if v == Relation::Implementation as i32 => "IMPLEMENTATION",
        v if v == Relation::Association as i32 => "ASSOCIATION",
        v if v == Relation::Aggregation as i32 => "AGGREGATION",
        v if v == Relation::Composition as i32 => "COMPOSITION",
        _ => "NONE",
    };

    let multiplicity_p = relation
        .multiplicity_p
        .as_ref()
        .map(proto_multiplicity_to_json)
        .unwrap_or_else(|| serde_json::json!(null));

    let multiplicity_c = relation
        .multiplicity_c
        .as_ref()
        .map(proto_multiplicity_to_json)
        .unwrap_or_else(|| serde_json::json!(null));

    serde_json::json!({
        "target_class_id": relation.target_class_id,
        "relation": relation_type,
        "multiplicity_p": multiplicity_p,
        "multiplicity_c": multiplicity_c,
        "role_name_p": relation.role_name_p,
        "role_name_c": relation.role_name_c
    })
}

fn proto_multiplicity_to_json(multiplicity: &Multiplicity) -> serde_json::Value {
    serde_json::json!({
        "lower": multiplicity.lower,
        "upper": multiplicity.upper
    })
}
