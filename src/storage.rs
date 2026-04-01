use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::models::*;

pub struct Storage {
    pool: PgPool,
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn init_schema(&self) -> Result<()> {
        let schema = r#"
            CREATE EXTENSION IF NOT EXISTS vector;
            CREATE EXTENSION IF NOT EXISTS pg_trgm;

            CREATE TABLE IF NOT EXISTS memories (
                id UUID PRIMARY KEY,
                network_type TEXT NOT NULL CHECK (network_type IN ('world', 'experience', 'opinion', 'observation')),
                content TEXT NOT NULL,
                embedding vector(768),
                entities JSONB NOT NULL DEFAULT '[]',
                confidence REAL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE IF NOT EXISTS edges (
                id UUID PRIMARY KEY,
                source_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                target_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                edge_type TEXT NOT NULL CHECK (edge_type IN ('temporal', 'semantic', 'entity', 'causal')),
                weight REAL NOT NULL DEFAULT 1.0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_memories_network ON memories(network_type);
            CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
            CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
            CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
        "#;

        sqlx::query(schema).execute(&self.pool).await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_fts ON memories USING GIN (to_tsvector('english', content))",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_embedding ON memories USING hnsw (embedding vector_cosine_ops)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn store_memory(
        &self,
        id: Uuid,
        network: NetworkType,
        content: &str,
        embedding: &[f32],
        entities: &[String],
        confidence: Option<f32>,
    ) -> Result<()> {
        let embed_str = format_vector(embedding);
        let entities_json = serde_json::to_value(entities)?;

        sqlx::query(
            r#"INSERT INTO memories (id, network_type, content, embedding, entities, confidence, created_at, updated_at)
               VALUES ($1, $2, $3, $4::vector, $5, $6, NOW(), NOW())"#,
        )
        .bind(id)
        .bind(network.as_str())
        .bind(content)
        .bind(&embed_str)
        .bind(entities_json)
        .bind(confidence)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn store_edge(
        &self,
        source_id: Uuid,
        target_id: Uuid,
        edge_type: EdgeType,
        weight: f32,
    ) -> Result<()> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO edges (id, source_id, target_id, edge_type, weight, created_at) VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(id)
        .bind(source_id)
        .bind(target_id)
        .bind(edge_type.as_str())
        .bind(weight)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn search_semantic(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        let embed_str = format_vector(query_embedding);
        let rows = sqlx::query(
            r#"SELECT id, network_type, content, embedding::text AS embedding_text,
                      entities, confidence, created_at, updated_at,
                      CAST(1 - (embedding <=> $1::vector) AS double precision) AS score
               FROM memories
               WHERE embedding IS NOT NULL
               ORDER BY embedding <=> $1::vector
               LIMIT $2"#,
        )
        .bind(&embed_str)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let score: f64 = row.try_get("score").unwrap_or(0.0);
            results.push(ScoredMemory {
                memory: row_to_memory(&row)?,
                score,
            });
        }
        Ok(results)
    }

    pub async fn search_keyword(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ScoredMemory>> {
        let tsquery: String = query
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| format!("{}:*", w.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" | ");

        if tsquery.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            r#"SELECT id, network_type, content, embedding::text AS embedding_text,
                      entities, confidence, created_at, updated_at,
                      CAST(ts_rank(to_tsvector('english', content), to_tsquery($1)) AS double precision) AS score
               FROM memories
               WHERE to_tsvector('english', content) @@ to_tsquery($1)
               ORDER BY score DESC
               LIMIT $2"#,
        )
        .bind(&tsquery)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let score: f64 = row.try_get("score").unwrap_or(0.0);
            results.push(ScoredMemory {
                memory: row_to_memory(&row)?,
                score,
            });
        }
        Ok(results)
    }

    pub async fn search_temporal(&self, limit: usize) -> Result<Vec<ScoredMemory>> {
        let rows = sqlx::query(
            r#"SELECT id, network_type, content, embedding::text AS embedding_text,
                      entities, confidence, created_at, updated_at,
                      CAST(EXP(-EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400.0) AS double precision) AS score
               FROM memories
               ORDER BY created_at DESC
               LIMIT $1"#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let score: f64 = row.try_get("score").unwrap_or(0.0);
            results.push(ScoredMemory {
                memory: row_to_memory(&row)?,
                score,
            });
        }
        Ok(results)
    }

    pub async fn get_neighbors(&self, memory_id: Uuid) -> Result<Vec<(Uuid, f32)>> {
        let rows = sqlx::query(
            "SELECT target_id, weight FROM edges WHERE source_id = $1",
        )
        .bind(memory_id)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let target_id: Uuid = row.get("target_id");
            let weight: f32 = row.get("weight");
            results.push((target_id, weight));
        }
        Ok(results)
    }

    pub async fn get_memory(&self, id: Uuid) -> Result<Option<MemoryUnit>> {
        let row = sqlx::query(
            "SELECT id, network_type, content, embedding::text AS embedding_text, entities, confidence, created_at, updated_at FROM memories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_memory(&row)?)),
            None => Ok(None),
        }
    }

    pub async fn update_confidence(&self, id: Uuid, new_confidence: f32) -> Result<()> {
        sqlx::query(
            "UPDATE memories SET confidence = $1, updated_at = NOW() WHERE id = $2 AND network_type = 'opinion'",
        )
        .bind(new_confidence)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_opinions_by_entities(
        &self,
        entities: &[String],
    ) -> Result<Vec<MemoryUnit>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for entity in entities {
            let pattern = format!(
                "%\"{}\"%",
                entity.replace('%', "\\%").replace('_', "\\_")
            );
            let rows = sqlx::query(
                r#"SELECT id, network_type, content, embedding::text AS embedding_text,
                          entities, confidence, created_at, updated_at
                   FROM memories
                   WHERE network_type = 'opinion' AND entities::text LIKE $1"#,
            )
            .bind(&pattern)
            .fetch_all(&self.pool)
            .await?;

            for row in rows {
                results.push(row_to_memory(&row)?);
            }
        }

        Ok(results)
    }
}

fn format_vector(v: &[f32]) -> String {
    let inner: Vec<String> = v.iter().map(|f| f.to_string()).collect();
    format!("[{}]", inner.join(","))
}

fn parse_vector(s: &str) -> Result<Vec<f32>> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|x| {
            x.trim()
                .parse::<f32>()
                .map_err(|e| anyhow!("Failed to parse vector element: {}", e))
        })
        .collect()
}

fn row_to_memory(row: &sqlx::postgres::PgRow) -> Result<MemoryUnit> {
    let id: Uuid = row.get("id");
    let network_type_str: String = row.get("network_type");
    let network = NetworkType::from_str(&network_type_str)
        .ok_or_else(|| anyhow!("Unknown network type: {}", network_type_str))?;
    let content: String = row.get("content");
    let embedding_text: Option<String> = row.try_get("embedding_text").ok().flatten();
    let embedding = embedding_text
        .as_deref()
        .map(parse_vector)
        .transpose()?
        .unwrap_or_default();
    let entities_value: serde_json::Value = row.get("entities");
    let entities: Vec<String> = serde_json::from_value(entities_value)?;
    let confidence: Option<f32> = row.try_get("confidence").ok().flatten();
    let created_at: DateTime<Utc> = row.get("created_at");
    let updated_at: DateTime<Utc> = row.get("updated_at");

    Ok(MemoryUnit {
        id,
        network,
        content,
        embedding,
        entities,
        confidence,
        created_at,
        updated_at,
    })
}
