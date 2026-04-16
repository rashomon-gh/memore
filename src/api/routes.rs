//! REST API routes and handlers for the memory visualization dashboard.
//!
//! Provides JSON endpoints for accessing memories, graph data, statistics,
//! and entity information from the web interface.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post},
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
        .route("/api/chats", get(list_chats))
        .route("/api/chats/:id", get(get_chat))
        .route("/api/chats/:id", delete(delete_chat))
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

    let chat_id = match req.chat_id {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4();
            let title: String = req.message.chars().take(50).collect();
            let title = if req.message.chars().count() > 50 {
                format!("{}...", title)
            } else {
                title
            };
            state.storage.create_chat(id, &title).await.map_err(|e| {
                tracing::error!("Create chat error: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            id
        }
    };

    state
        .storage
        .add_chat_message(Uuid::new_v4(), chat_id, "user", &req.message)
        .await
        .map_err(|e| {
            tracing::error!("Store message error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let memories = state
        .cara
        .retain(&req.message, Some(chat_id))
        .await
        .map_err(|e| {
            tracing::error!("Retain error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let (response, opinions) = state.cara.reflect(&req.message, 2000, Some(chat_id)).await.map_err(|e| {
        tracing::error!("Reflect error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    state
        .storage
        .add_chat_message(Uuid::new_v4(), chat_id, "assistant", &response)
        .await
        .map_err(|e| {
            tracing::error!("Store message error: {}", e);
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
        chat_id,
        response,
        new_memories,
        opinions: chat_opinions,
    }))
}

/// GET /api/chats - List all chat sessions.
pub async fn list_chats(
    State(state): State<ApiState>,
) -> Result<Json<Vec<ChatSummary>>, StatusCode> {
    let chats = state.storage.list_chats().await.map_err(|e| {
        tracing::error!("List chats error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let summaries: Vec<ChatSummary> = chats
        .into_iter()
        .map(|(id, title, created_at, updated_at)| ChatSummary {
            id,
            title,
            created_at,
            updated_at,
        })
        .collect();

    Ok(Json(summaries))
}

/// GET /api/chats/:id - Get a chat with messages and linked memories.
pub async fn get_chat(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ChatDetail>, StatusCode> {
    let chats = state.storage.list_chats().await.map_err(|e| {
        tracing::error!("Get chat error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let (title, created_at, updated_at) = chats
        .iter()
        .find(|(cid, _, _, _)| cid == &id)
        .map(|(_, t, ca, ua)| (t.clone(), *ca, *ua))
        .ok_or(StatusCode::NOT_FOUND)?;

    let messages = state.storage.get_chat_messages(id).await.map_err(|e| {
        tracing::error!("Get chat messages error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let chat_messages: Vec<ChatMessageEntry> = messages
        .into_iter()
        .map(|(_, role, content, created_at)| ChatMessageEntry {
            role,
            content,
            created_at,
        })
        .collect();

    let memories = state.storage.get_memories_by_chat(id).await.map_err(|e| {
        tracing::error!("Get chat memories error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let chat_memories: Vec<ChatMemory> = memories
        .into_iter()
        .map(|m| ChatMemory {
            id: m.id,
            network: m.network.as_str().to_string(),
            content: m.content,
            entities: m.entities,
            confidence: m.confidence,
        })
        .collect();

    Ok(Json(ChatDetail {
        id,
        title,
        created_at,
        updated_at,
        messages: chat_messages,
        memories: chat_memories,
    }))
}

/// DELETE /api/chats/:id - Delete a chat and all its associated data.
pub async fn delete_chat(
    State(state): State<ApiState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    state.storage.delete_chat(id).await.map_err(|e| {
        tracing::error!("Delete chat error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}
