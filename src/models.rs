use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    World,
    Experience,
    Opinion,
    Observation,
}

impl NetworkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Experience => "experience",
            Self::Opinion => "opinion",
            Self::Observation => "observation",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "world" => Some(Self::World),
            "experience" => Some(Self::Experience),
            "opinion" => Some(Self::Opinion),
            "observation" => Some(Self::Observation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    Temporal,
    Semantic,
    Entity,
    Causal,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Temporal => "temporal",
            Self::Semantic => "semantic",
            Self::Entity => "entity",
            Self::Causal => "causal",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "temporal" => Some(Self::Temporal),
            "semantic" => Some(Self::Semantic),
            "entity" => Some(Self::Entity),
            "causal" => Some(Self::Causal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUnit {
    pub id: Uuid,
    pub network: NetworkType,
    pub content: String,
    pub embedding: Vec<f32>,
    pub entities: Vec<String>,
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub edge_type: EdgeType,
    pub weight: f32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub background: String,
    pub skepticism: u8,
    pub literalism: u8,
    pub empathy: u8,
    pub bias_strength: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFact {
    pub content: String,
    pub network: NetworkType,
    pub entities: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub links: Vec<FactLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactLink {
    pub target_fact_index: usize,
    pub edge_type: EdgeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFacts {
    pub facts: Vec<ExtractedFact>,
}

#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub memory: MemoryUnit,
    pub score: f64,
}
