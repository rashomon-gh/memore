//! REST API routes and handlers for the memory visualization dashboard.
//!
//! Provides JSON endpoints for accessing memories, graph data, statistics,
//! and entity information from the web interface.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::cara::CaraPipeline;
use crate::storage::Storage;
use crate::models::NetworkType;
use crate::api::models::*;

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
    pub cara: Arc<CaraPipeline>,
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
        .route("/api/chat", post(chat))
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

/// POST /api/chat - Send a message and get a response with new memories.
pub async fn chat(
    State(state): State<ApiState>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    if req.message.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let memories = state
        .cara
        .retain(&req.message)
        .await
        .map_err(|e| {
            tracing::error!("Retain error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let (response, opinions) = state.cara.reflect(&req.message, 2000).await.map_err(|e| {
        tracing::error!("Reflect error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let new_memories: Vec<ChatMemory> = memories
        .into_iter()
        .map(|m| ChatMemory {
            id: m.id,
            network: m.network.as_str().to_string(),
            content: m.content,
            entities: m.entities,
            confidence: m.confidence,
        })
        .collect();

    let chat_opinions: Vec<ChatMemory> = opinions
        .into_iter()
        .map(|m| ChatMemory {
            id: m.id,
            network: m.network.as_str().to_string(),
            content: m.content,
            entities: m.entities,
            confidence: m.confidence,
        })
        .collect();

    Ok(Json(ChatResponse {
        response,
        new_memories,
        opinions: chat_opinions,
    }))
}
