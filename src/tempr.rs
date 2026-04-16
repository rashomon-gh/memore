//! TEMPR pipeline — **Retain** and **Recall** operations.
//!
//! # Retain
//!
//! Parses a conversation through the LLM to extract self-contained facts,
//! classifies each into one of the four memory networks, generates embeddings,
//! stores them in PostgreSQL, creates graph edges between related facts, and
//! reinforces existing opinions that share entities with new facts.
//!
//! # Recall
//!
//! Executes four retrieval strategies in parallel (semantic, keyword, temporal,
//! graph), then merges results with **Reciprocal Rank Fusion** and trims to a
//! token budget.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use chrono::Utc;
use uuid::Uuid;

use crate::llm::{ChatMessage, LLMClient};
use crate::models::*;
use crate::storage::Storage;

/// RRF constant `k` used in the fusion formula `1 / (k + rank + 1)`.
const RRF_K: u64 = 60;
/// Maximum number of hops during spreading-activation graph traversal.
const GRAPH_MAX_HOPS: usize = 3;
/// Activation decay factor per hop (0.0–1.0).
const GRAPH_DECAY: f64 = 0.7;
/// Maximum results returned by each individual search strategy.
const SEARCH_LIMIT: usize = 20;

/// The TEMPR (Temporal-Entity Memory Processing & Retrieval) pipeline.
///
/// Owns the [`LLMClient`] and [`Storage`] and exposes the two primary
/// operations: [`retain`](Self::retain) and [`recall`](Self::recall).
pub struct TemprPipeline {
    llm: LLMClient,
    storage: Storage,
    embedding_dim: usize,
}

impl TemprPipeline {
    /// Creates a new pipeline from the given LLM client, storage handle, and
    /// embedding dimensionality.
    pub fn new(llm: LLMClient, storage: Storage, embedding_dim: usize) -> Self {
        Self {
            llm,
            storage,
            embedding_dim,
        }
    }

    /// Returns a reference to the inner [`LLMClient`].
    pub fn llm(&self) -> &LLMClient {
        &self.llm
    }

    /// Returns a reference to the inner [`Storage`].
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Extracts, classifies, embeds, and stores facts from a conversation.
    ///
    /// After storing all facts and their inter-links, any existing opinion
    /// memories that share entities with the new facts have their confidence
    /// bumped by 0.05 (capped at 1.0).
    ///
    /// Returns the list of newly created [`MemoryUnit`]s.
    pub async fn retain(&self, conversation: &str, chat_id: Option<Uuid>) -> Result<Vec<MemoryUnit>> {
        let system_prompt = r#"You are a memory extraction system. Analyze the conversation and extract structured facts.

For each fact, provide a JSON object with:
- "content": A self-contained narrative fact (understandable without additional context)
- "network": One of "world" (objective facts about the external world), "experience" (biographical info about the agent, written in first person), "opinion" (subjective judgments or preferences), or "observation" (preference-neutral synthesized summaries of entities)
- "entities": Array of named entities mentioned in the fact
- "confidence": (opinions only) A score from 0.0 to 1.0 indicating strength of the judgment
- "links": Array of relationships to other extracted facts, each with "target_fact_index" (0-based index) and "edge_type" (one of "temporal", "semantic", "entity", "causal")

Return ONLY a JSON object: {"facts": [...]}
If no facts can be extracted, return: {"facts": []}"#;

        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: conversation.into(),
            },
        ];

        let response = self
            .llm
            .chat_completion(messages, Some(0.3), Some(4096))
            .await?;
        let json_str = extract_json(&response);
        let extracted: ExtractedFacts = serde_json::from_str(json_str).map_err(|e| {
            anyhow!(
                "Failed to parse extracted facts: {} — raw response: {}",
                e,
                response
            )
        })?;

        let mut stored = Vec::new();
        let mut fact_ids: Vec<Uuid> = Vec::new();

        for fact in &extracted.facts {
            let id = Uuid::new_v4();
            fact_ids.push(id);

            let embedding = match self.llm.embed_single(fact.content.clone()).await {
                Ok(emb) => emb,
                Err(e) => {
                    tracing::warn!("Failed to generate embedding, using zero vector: {}", e);
                    vec![0.0f32; self.embedding_dim]
                }
            };

            self.storage
                .store_memory(
                    id,
                    fact.network,
                    &fact.content,
                    &embedding,
                    &fact.entities,
                    fact.confidence,
                    chat_id,
                )
                .await?;

            stored.push(MemoryUnit {
                id,
                network: fact.network,
                content: fact.content.clone(),
                embedding,
                entities: fact.entities.clone(),
                confidence: fact.confidence,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }

        for (i, fact) in extracted.facts.iter().enumerate() {
            for link in &fact.links {
                if link.target_fact_index < fact_ids.len() && link.target_fact_index != i {
                    let weight = default_edge_weight(&link.edge_type);
                    self.storage
                        .store_edge(
                            fact_ids[i],
                            fact_ids[link.target_fact_index],
                            link.edge_type,
                            weight,
                        )
                        .await?;
                }
            }
        }

        for fact in &extracted.facts {
            if !fact.entities.is_empty() {
                let related = self
                    .storage
                    .find_opinions_by_entities(&fact.entities)
                    .await?;
                for opinion in &related {
                    if let Some(current_conf) = opinion.confidence {
                        let new_conf = (current_conf + 0.05).min(1.0);
                        self.storage.update_confidence(opinion.id, new_conf).await?;
                        tracing::info!(
                            "Reinforced opinion {} confidence: {:.2} -> {:.2}",
                            opinion.id,
                            current_conf,
                            new_conf
                        );
                    }
                }
            }
        }

        Ok(stored)
    }

    /// Retrieves memories relevant to a query within a token budget.
    ///
    /// Runs semantic, keyword, and temporal searches in parallel, then
    /// performs spreading activation from the top results. All four ranked
    /// lists are fused with RRF, deduplicated, and trimmed to fit
    /// `token_budget` (estimated at ~4 characters per token).
    pub async fn recall(&self, query: &str, token_budget: usize) -> Result<Vec<ScoredMemory>> {
        let query_embedding = self.llm.embed_single(query.to_string()).await?;

        let (semantic, keyword, temporal) = tokio::try_join!(
            self.storage.search_semantic(&query_embedding, SEARCH_LIMIT),
            self.storage.search_keyword(query, SEARCH_LIMIT),
            self.storage.search_temporal(SEARCH_LIMIT),
        )?;

        let seed_ids: Vec<Uuid> = semantic
            .iter()
            .chain(keyword.iter())
            .map(|sm| sm.memory.id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let graph = self.spreading_activation(&seed_ids).await;

        let rankings = vec![
            semantic.iter().map(|sm| (sm.memory.id, sm.score)).collect(),
            keyword.iter().map(|sm| (sm.memory.id, sm.score)).collect(),
            temporal.iter().map(|sm| (sm.memory.id, sm.score)).collect(),
            graph,
        ];

        let fused = reciprocal_rank_fusion(&rankings, RRF_K);

        let mut results = Vec::new();
        let mut tokens_used = 0;
        let mut seen = HashSet::new();

        for (id, score) in &fused {
            if seen.contains(id) {
                continue;
            }
            seen.insert(*id);

            if let Some(memory) = self.storage.get_memory(*id).await? {
                let tokens = estimate_tokens(&memory.content);
                if tokens_used + tokens > token_budget {
                    break;
                }
                tokens_used += tokens;
                results.push(ScoredMemory {
                    memory,
                    score: *score,
                });
            }
        }

        Ok(results)
    }

    /// Spreading-activation graph traversal starting from `seed_ids`.
    ///
    /// Traverses up to [`GRAPH_MAX_HOPS`] hops, decaying activation by
    /// [`GRAPH_DECAY`] and the edge weight at each step. Nodes receiving
    /// activation below 0.1 are pruned.
    async fn spreading_activation(&self, seed_ids: &[Uuid]) -> Vec<(Uuid, f64)> {
        let mut activated: HashMap<Uuid, f64> = HashMap::new();
        let mut frontier: Vec<(Uuid, f64)> = seed_ids.iter().map(|id| (*id, 1.0)).collect();

        for _ in 0..GRAPH_MAX_HOPS {
            let mut next_frontier = Vec::new();
            for (node_id, activation) in &frontier {
                let current = activated.entry(*node_id).or_default();
                if *current >= *activation {
                    continue;
                }
                *current = *activation;

                match self.storage.get_neighbors(*node_id).await {
                    Ok(neighbors) => {
                        for (neighbor_id, edge_weight) in neighbors {
                            let new_activation = activation * edge_weight as f64 * GRAPH_DECAY;
                            if new_activation > 0.1 {
                                next_frontier.push((neighbor_id, new_activation));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Graph traversal error: {}", e);
                    }
                }
            }
            frontier = next_frontier;
        }

        let mut results: Vec<_> = activated.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

/// Merges multiple ranked lists using the **Reciprocal Rank Fusion** formula:
///
/// `score(d) = Σ 1 / (k + rank_i + 1)`
///
/// where `k` is a smoothing constant (typically 60).
fn reciprocal_rank_fusion(rankings: &[Vec<(Uuid, f64)>], k: u64) -> Vec<(Uuid, f64)> {
    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for ranking in rankings {
        for (rank, (id, _score)) in ranking.iter().enumerate() {
            *scores.entry(*id).or_default() += 1.0 / (k as f64 + rank as f64 + 1.0);
        }
    }
    let mut results: Vec<_> = scores.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

/// Returns a default edge weight depending on the semantic meaning of the
/// edge type. Causal edges are strongest; semantic edges are weakest.
fn default_edge_weight(edge_type: &EdgeType) -> f32 {
    match edge_type {
        EdgeType::Temporal => 0.8,
        EdgeType::Semantic => 0.7,
        EdgeType::Entity => 0.9,
        EdgeType::Causal => 0.95,
    }
}

/// Best-effort extraction of a JSON object from an LLM response that may be
/// wrapped in Markdown code fences.
fn extract_json(text: &str) -> &str {
    if let Some(start) = text.find("```json") {
        let start = start + 7;
        if let Some(end) = text[start..].find("```") {
            return text[start..start + end].trim();
        }
    }
    if let Some(start) = text.find("```") {
        let start = start + 3;
        if let Some(end) = text[start..].find("```") {
            return text[start..start + end].trim();
        }
    }
    if let Some(start) = text.find('{')
        && let Some(end) = text.rfind('}')
    {
        return &text[start..=end];
    }
    text
}

/// Rough token estimator: ~4 characters per token.
fn estimate_tokens(text: &str) -> usize {
    (text.len() as f64 / 4.0).ceil() as usize
}
