//! REST API routes and handlers for the memory visualization dashboard.
//!
//! Provides JSON endpoints for accessing memories, graph data, statistics,
//! and entity information from the web interface.

use axum::{
    extract::{Path, Query, State, Multipart},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::storage::Storage;
use crate::models::NetworkType;
use crate::api::models::*;
use crate::files::{processor::FileProcessor, FileType};

/// Query parameters for memory listing endpoint.
#[derive(Debug, Deserialize)]
pub struct MemoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub network: Option<String>,
    pub search: Option<String>,
    pub entities: Option<String>,
    pub min_confidence: Option<f32>,
}

/// Query parameters for graph data endpoint.
#[derive(Debug, Deserialize)]
pub struct GraphQuery {
    pub network: Option<String>,
    pub limit: Option<usize>,
}

/// Shared application state for API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub storage: Arc<Storage>,
    pub llm: Arc<crate::llm::LLMClient>,
    pub embedding_dim: usize,
}

/// Create the API router with all endpoints.
pub fn create_api_router() -> Router<ApiState> {
    Router::new()
        .route("/api/memories", get(list_memories))
        .route("/api/memories/:id", get(get_memory))
        .route("/api/graph", get(get_graph))
        .route("/api/entities", get(list_entities))
        .route("/api/stats", get(get_stats))
        .route("/api/networks/:network_type", get(get_by_network))
        .route("/api/files/upload", post(upload_file))
        .route("/api/files", get(list_files))
        .route("/api/files/:id", get(get_file))
}

/// GET /api/memories - List and search memories with pagination.
pub async fn list_memories(
    State(state): State<ApiState>,
    Query(params): Query<MemoryQuery>,
) -> Result<Json<MemoryListResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    let memories = if let Some(search_query) = &params.search {
        // Use keyword search for text queries
        state.storage
            .search_keyword(search_query, limit * 2)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .into_iter()
            .map(|sm| sm.memory)
            .collect()
    } else if let Some(network_str) = &params.network {
        // Filter by network type
        let network_type = NetworkType::from_str(network_str)
            .ok_or(StatusCode::BAD_REQUEST)?;

        state.storage
            .get_memories_by_network(network_type, limit, offset)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        // Get all memories with pagination
        state.storage
            .get_all_memories(limit, offset)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let total = memories.len();
    let api_memories: Vec<ApiMemory> = memories
        .into_iter()
        .map(|m| ApiMemory {
            id: m.id,
            network: m.network.as_str().to_string(),
            content: m.content,
            entities: m.entities,
            confidence: m.confidence,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
        .collect();

    Ok(Json(MemoryListResponse {
        memories: api_memories,
        total,
        limit,
        offset,
    }))
}

/// GET /api/memories/:id - Get single memory with neighbors.
pub async fn get_memory(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MemoryDetail>, StatusCode> {
    let memory = state
        .storage
        .get_memory(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let neighbors = state.storage
        .get_neighbors_detailed(id, 10)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let api_memory = ApiMemory {
        id: memory.id,
        network: memory.network.as_str().to_string(),
        content: memory.content,
        entities: memory.entities,
        confidence: memory.confidence,
        created_at: memory.created_at,
        updated_at: memory.updated_at,
    };

    let neighbor_memories: Vec<NeighborMemory> = neighbors
        .into_iter()
        .map(|(m, edge_type, weight)| NeighborMemory {
            memory: ApiMemory {
                id: m.id,
                network: m.network.as_str().to_string(),
                content: m.content,
                entities: m.entities,
                confidence: m.confidence,
                created_at: m.created_at,
                updated_at: m.updated_at,
            },
            edge_type: edge_type.as_str().to_string(),
            weight,
        })
        .collect();

    Ok(Json(MemoryDetail {
        memory: api_memory,
        neighbors: neighbor_memories,
    }))
}

/// GET /api/graph - Export graph data for Cytoscape visualization.
pub async fn get_graph(
    State(state): State<ApiState>,
    Query(params): Query<GraphQuery>,
) -> Result<Json<GraphData>, StatusCode> {
    let limit = params.limit.unwrap_or(200);

    // Get memories (optionally filtered by network)
    let memories = if let Some(network_str) = &params.network {
        let network_type = NetworkType::from_str(network_str)
            .ok_or(StatusCode::BAD_REQUEST)?;
        state.storage
            .get_memories_by_network(network_type, limit, 0)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        state.storage
            .get_all_memories(limit, 0)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    // Get all edges
    let edges = state.storage
        .get_all_edges()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create filtered edge set (only include edges between visible memories)
    let memory_ids: std::collections::HashSet<Uuid> = memories
        .iter()
        .map(|m| m.id)
        .collect();

    let filtered_edges: Vec<_> = edges
        .into_iter()
        .filter(|e| memory_ids.contains(&e.source_id) && memory_ids.contains(&e.target_id))
        .collect();

    // Convert to Cytoscape format
    let nodes: Vec<GraphNode> = memories
        .into_iter()
        .map(|m| {
            // Safely truncate content to 50 characters (not bytes)
            let content_preview = if m.content.chars().count() > 50 {
                let truncated: String = m.content.chars().take(50).collect();
                format!("{}...", truncated)
            } else {
                m.content.clone()
            };

            GraphNode {
                data: NodeData {
                    id: m.id.to_string(),
                    label: content_preview,
                    network: m.network.as_str().to_string(),
                    entities: m.entities,
                    confidence: m.confidence,
                },
            }
        })
        .collect();

    let graph_edges: Vec<GraphEdge> = filtered_edges
        .into_iter()
        .map(|e| GraphEdge {
            data: EdgeData {
                id: e.id.to_string(),
                source: e.source_id.to_string(),
                target: e.target_id.to_string(),
                edge_type: e.edge_type.as_str().to_string(),
                weight: e.weight,
            },
        })
        .collect();

    Ok(Json(GraphData {
        nodes,
        edges: graph_edges,
    }))
}

/// GET /api/entities - List all unique entities.
pub async fn list_entities(
    State(state): State<ApiState>,
) -> Result<Json<EntityList>, StatusCode> {
    let entities = state.storage
        .get_all_entities()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = entities.len();
    Ok(Json(EntityList {
        entities,
        total,
    }))
}

/// GET /api/stats - Get analytics statistics.
pub async fn get_stats(
    State(state): State<ApiState>,
) -> Result<Json<MemoryStats>, StatusCode> {
    let stats = state.storage
        .get_statistics()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(stats))
}

/// GET /api/networks/:network_type - Get memories by network type.
pub async fn get_by_network(
    State(state): State<ApiState>,
    Path(network_type): Path<String>,
) -> Result<Json<Vec<ApiMemory>>, StatusCode> {
    let network = NetworkType::from_str(&network_type)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let memories = state.storage
        .get_memories_by_network(network, 100, 0)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let api_memories: Vec<ApiMemory> = memories
        .into_iter()
        .map(|m| ApiMemory {
            id: m.id,
            network: m.network.as_str().to_string(),
            content: m.content,
            entities: m.entities,
            confidence: m.confidence,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
        .collect();

    Ok(Json(api_memories))
}

/// POST /api/files/upload - Upload and process a file.
pub async fn upload_file(
    State(state): State<ApiState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Create a temporary directory for uploads
    let temp_dir = std::env::temp_dir();

    let mut filename = String::new();
    let mut file_content = Vec::new();
    let mut temp_file_path = temp_dir.join(format!("hindsight_upload_{}", Uuid::new_v4()));

    // Process multipart form data
    while let Some(mut field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "file" {
            filename = field.file_name()
                .unwrap_or("unknown")
                .to_string();

            // Update temp file path to include original extension
            let file_extension = std::path::Path::new(&filename)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("bin");
            temp_file_path = temp_dir.join(format!("hindsight_upload_{}.{}", Uuid::new_v4(), file_extension));

            // Read the field data into bytes
            use futures_util::stream::StreamExt;
            let mut content = Vec::new();
            while let Some(chunk) = field.next().await {
                let chunk = chunk.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                content.extend_from_slice(&chunk);
            }
            file_content = content;
        }
    }

    if filename.is_empty() || file_content.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Write uploaded content to temp file
    tokio::fs::write(&temp_file_path, &file_content)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Process the file
    let processor = FileProcessor::new(state.storage.clone(), state.llm.clone(), state.embedding_dim);

    match processor.process_file(temp_file_path.clone()).await {
        Ok(result) => {
            // Clean up temp file
            let _ = tokio::fs::remove_file(&temp_file_path).await;

            Ok(Json(json!({
                "status": "success",
                "file_id": result.file_metadata.id,
                "filename": result.file_metadata.filename,
                "memories_created": result.total_memories_created,
                "processing_time_ms": result.processing_time_ms,
                "file_type": match result.file_metadata.file_type {
                    FileType::PDF => "pdf",
                    FileType::Markdown => "markdown",
                    FileType::Text => "text",
                    FileType::Unknown => "unknown",
                }
            })))
        }
        Err(e) => {
            // Clean up temp file on error
            let _ = tokio::fs::remove_file(&temp_file_path).await;

            tracing::error!("Failed to process uploaded file: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// GET /api/files - List all processed files.
pub async fn list_files(
    State(state): State<ApiState>,
) -> Result<Json<Vec<FileMetadataResponse>>, StatusCode> {
    let files = state.storage
        .get_all_files()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let response: Vec<FileMetadataResponse> = files
        .into_iter()
        .map(|f| FileMetadataResponse {
            id: f.id,
            filename: f.filename,
            path: f.path.to_string_lossy().to_string(),
            file_type: match f.file_type {
                FileType::PDF => "pdf".to_string(),
                FileType::Markdown => "markdown".to_string(),
                FileType::Text => "text".to_string(),
                FileType::Unknown => "unknown".to_string(),
            },
            size_bytes: f.size_bytes,
            processed_at: f.processed_at,
            content_length: f.content_length,
            chunk_count: f.chunk_count,
        })
        .collect();

    Ok(Json(response))
}

/// GET /api/files/:id - Get specific file metadata.
pub async fn get_file(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<FileMetadataResponse>, StatusCode> {
    let file_uuid = Uuid::parse_str(&id)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // For now, return all files and find by id (inefficient but works)
    let files = state.storage
        .get_all_files()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let file = files
        .into_iter()
        .find(|f| f.id == id || Uuid::parse_str(&f.id).map(|u| u == file_uuid).unwrap_or(false))
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(FileMetadataResponse {
        id: file.id,
        filename: file.filename,
        path: file.path.to_string_lossy().to_string(),
        file_type: match file.file_type {
            FileType::PDF => "pdf".to_string(),
            FileType::Markdown => "markdown".to_string(),
            FileType::Text => "text".to_string(),
            FileType::Unknown => "unknown".to_string(),
        },
        size_bytes: file.size_bytes,
        processed_at: file.processed_at,
        content_length: file.content_length,
        chunk_count: file.chunk_count,
    }))
}

/// File metadata response for API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadataResponse {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub file_type: String,
    pub size_bytes: i64,
    pub processed_at: chrono::DateTime<chrono::Utc>,
    pub content_length: i32,
    pub chunk_count: i32,
}
