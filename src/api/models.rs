//! API-specific request and response models for the web dashboard.
//!
//! These models are designed for JSON serialization and represent the
//! public API contract for the web visualization interface.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Memory unit response for API consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMemory {
    pub id: Uuid,
    pub network: String,
    pub content: String,
    pub entities: Vec<String>,
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Graph node representation for Cytoscape.js.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub data: NodeData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    pub id: String,
    pub label: String,
    pub network: String,
    pub entities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Graph edge representation for Cytoscape.js.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub data: EdgeData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeData {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub weight: f32,
}

/// Complete graph response with nodes and edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Statistics response for analytics dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_memories: usize,
    pub total_edges: usize,
    pub memories_by_network: NetworkStats,
    pub edges_by_type: EdgeTypeStats,
    pub top_entities: Vec<EntityStat>,
    pub recent_memories: usize,
    pub average_confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub world: usize,
    pub experience: usize,
    pub opinion: usize,
    pub observation: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeStats {
    pub temporal: usize,
    pub semantic: usize,
    pub entity: usize,
    pub causal: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityStat {
    pub entity: String,
    pub count: usize,
}

/// Memory list response with pagination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResponse {
    pub memories: Vec<ApiMemory>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Memory detail response including neighbors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDetail {
    pub memory: ApiMemory,
    pub neighbors: Vec<NeighborMemory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborMemory {
    pub memory: ApiMemory,
    pub edge_type: String,
    pub weight: f32,
}

/// Entity list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityList {
    pub entities: Vec<String>,
    pub total: usize,
}
