//! PostgreSQL storage layer backed by pgvector and full-text search.
//!
//! Provides CRUD operations for memory units and graph edges, plus four
//! retrieval strategies: semantic (HNSW cosine), keyword (GIN/BM25),
//! temporal (recency decay), and graph traversal (spreading activation).

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::models::*;

/// Database storage handle backed by a PostgreSQL connection pool.
pub struct Storage {
    pool: PgPool,
}

impl Storage {
    /// Connects to PostgreSQL and returns a new [`Storage`] instance.
    ///
    /// Does **not** run migrations — call [`init_schema`](Self::init_schema)
    /// afterwards.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// Creates the `memories`, `edges`, and `files` tables, installs the `vector`
    /// and `pg_trgm` extensions, and builds the HNSW and GIN indexes.
    ///
    /// Safe to call multiple times (uses `IF NOT EXISTS`).
    pub async fn init_schema(&self) -> Result<()> {
        let stmts: &[&str] = &[
            "CREATE EXTENSION IF NOT EXISTS vector",
            "CREATE EXTENSION IF NOT EXISTS pg_trgm",
            "CREATE TABLE IF NOT EXISTS memories (
                id UUID PRIMARY KEY,
                network_type TEXT NOT NULL CHECK (network_type IN ('world', 'experience', 'opinion', 'observation')),
                content TEXT NOT NULL,
                embedding vector(768),
                entities JSONB NOT NULL DEFAULT '[]',
                confidence REAL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS edges (
                id UUID PRIMARY KEY,
                source_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                target_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                edge_type TEXT NOT NULL CHECK (edge_type IN ('temporal', 'semantic', 'entity', 'causal')),
                weight REAL NOT NULL DEFAULT 1.0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE TABLE IF NOT EXISTS files (
                id UUID PRIMARY KEY,
                filename TEXT NOT NULL,
                path TEXT NOT NULL,
                file_type TEXT NOT NULL,
                size_bytes BIGINT NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                content_length INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS idx_memories_network ON memories(network_type)",
            "CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id)",
            "CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id)",
            "CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type)",
            "CREATE INDEX IF NOT EXISTS idx_memories_fts ON memories USING GIN (to_tsvector('english', content))",
            "CREATE INDEX IF NOT EXISTS idx_memories_embedding ON memories USING hnsw (embedding vector_cosine_ops)",
            "CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash)",
        ];

        for stmt in stmts {
            sqlx::query(stmt).execute(&self.pool).await?;
        }

        Ok(())
    }

    /// Inserts a new memory unit with its embedding and entity list.
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

    /// Inserts a directed edge between two memory units.
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

    /// Semantic search using cosine distance over HNSW-indexed embeddings.
    ///
    /// Returns up to `limit` results scored by `1 - cosine_distance`.
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

    /// Full-text keyword search using PostgreSQL `ts_rank` (BM25-like).
    ///
    /// Words shorter than 3 characters are ignored.
    pub async fn search_keyword(&self, query: &str, limit: usize) -> Result<Vec<ScoredMemory>> {
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

    /// Temporal search ordered by recency, scored with an exponential decay
    /// (half-life ≈ 1 day).
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

    /// Returns the outgoing neighbors (target ID + edge weight) of a memory
    /// unit for graph traversal.
    pub async fn get_neighbors(&self, memory_id: Uuid) -> Result<Vec<(Uuid, f32)>> {
        let rows = sqlx::query("SELECT target_id, weight FROM edges WHERE source_id = $1")
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

    /// Fetches a single memory unit by ID.
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

    /// Get all memories with pagination.
    pub async fn get_all_memories(&self, limit: usize, offset: usize) -> Result<Vec<MemoryUnit>> {
        let rows = sqlx::query(
            "SELECT id, network_type, content, embedding::text AS embedding_text, entities, confidence, created_at, updated_at
             FROM memories
             ORDER BY created_at DESC
             LIMIT $1 OFFSET $2"
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row_to_memory(&row)?);
        }
        Ok(results)
    }

    /// Get memories by network type with pagination.
    pub async fn get_memories_by_network(&self, network: NetworkType, limit: usize, offset: usize) -> Result<Vec<MemoryUnit>> {
        let rows = sqlx::query(
            "SELECT id, network_type, content, embedding::text AS embedding_text, entities, confidence, created_at, updated_at
             FROM memories
             WHERE network_type = $1
             ORDER BY created_at DESC
             LIMIT $2 OFFSET $3"
        )
        .bind(network.as_str())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row_to_memory(&row)?);
        }
        Ok(results)
    }

    /// Get all edges for graph visualization.
    pub async fn get_all_edges(&self) -> Result<Vec<Edge>> {
        let rows = sqlx::query(
            "SELECT id, source_id, target_id, edge_type, weight, created_at FROM edges"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let id: Uuid = row.get("id");
            let source_id: Uuid = row.get("source_id");
            let target_id: Uuid = row.get("target_id");
            let edge_type_str: String = row.get("edge_type");
            let edge_type = EdgeType::from_str(&edge_type_str)
                .ok_or_else(|| anyhow!("Unknown edge type: {}", edge_type_str))?;
            let weight: f32 = row.get("weight");
            let created_at: DateTime<Utc> = row.get("created_at");

            results.push(Edge {
                id,
                source_id,
                target_id,
                edge_type,
                weight,
                created_at,
            });
        }
        Ok(results)
    }

    /// Get all unique entities from memories.
    pub async fn get_all_entities(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT DISTINCT jsonb_array_elements_text(entities) AS entity
             FROM memories
             WHERE jsonb_array_length(entities) > 0
             ORDER BY entity"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut entities = Vec::new();
        for row in rows {
            let entity: String = row.get("entity");
            entities.push(entity);
        }
        Ok(entities)
    }

    /// Get detailed neighbors with full memory and edge information.
    pub async fn get_neighbors_detailed(&self, memory_id: Uuid, limit: usize) -> Result<Vec<(MemoryUnit, EdgeType, f32)>> {
        let rows = sqlx::query(
            r#"SELECT e.target_id, e.edge_type, e.weight,
                      m.id, m.network_type, m.content, m.embedding::text AS embedding_text,
                      m.entities, m.confidence, m.created_at, m.updated_at
               FROM edges e
               JOIN memories m ON e.target_id = m.id
               WHERE e.source_id = $1
               LIMIT $2"#
        )
        .bind(memory_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let edge_type_str: String = row.get("edge_type");
            let edge_type = EdgeType::from_str(&edge_type_str)
                .ok_or_else(|| anyhow!("Unknown edge type: {}", edge_type_str))?;
            let weight: f32 = row.get("weight");
            let memory = row_to_memory(&row)?;
            results.push((memory, edge_type, weight));
        }
        Ok(results)
    }

    /// Get statistics for analytics dashboard.
    pub async fn get_statistics(&self) -> Result<crate::api::models::MemoryStats> {
        use crate::api::models::*;

        // Get total counts
        let total_memories: i64 = sqlx::query("SELECT COUNT(*) FROM memories")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let total_edges: i64 = sqlx::query("SELECT COUNT(*) FROM edges")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        // Get counts by network
        let world_count: i64 = sqlx::query("SELECT COUNT(*) FROM memories WHERE network_type = 'world'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let experience_count: i64 = sqlx::query("SELECT COUNT(*) FROM memories WHERE network_type = 'experience'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let opinion_count: i64 = sqlx::query("SELECT COUNT(*) FROM memories WHERE network_type = 'opinion'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let observation_count: i64 = sqlx::query("SELECT COUNT(*) FROM memories WHERE network_type = 'observation'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        // Get edge type counts
        let temporal_count: i64 = sqlx::query("SELECT COUNT(*) FROM edges WHERE edge_type = 'temporal'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let semantic_count: i64 = sqlx::query("SELECT COUNT(*) FROM edges WHERE edge_type = 'semantic'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let entity_count: i64 = sqlx::query("SELECT COUNT(*) FROM edges WHERE edge_type = 'entity'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        let causal_count: i64 = sqlx::query("SELECT COUNT(*) FROM edges WHERE edge_type = 'causal'")
            .fetch_one(&self.pool)
            .await?
            .get("count");

        // Get top entities
        let entity_rows = sqlx::query(
            r#"SELECT jsonb_array_elements_text(entities) AS entity, COUNT(*) as count
               FROM memories
               WHERE jsonb_array_length(entities) > 0
               GROUP BY entity
               ORDER BY count DESC
               LIMIT 20"#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut top_entities = Vec::new();
        for row in entity_rows {
            let entity: String = row.get("entity");
            let count: i64 = row.get("count");
            top_entities.push(EntityStat {
                entity,
                count: count as usize,
            });
        }

        // Get recent memories (last 24 hours)
        let recent_memories: i64 = sqlx::query(
            "SELECT COUNT(*) FROM memories WHERE created_at > NOW() - INTERVAL '24 hours'"
        )
        .fetch_one(&self.pool)
        .await?
        .get("count");

        // Get average confidence for opinions
        let avg_confidence: Option<f32> = sqlx::query("SELECT AVG(confidence) FROM memories WHERE network_type = 'opinion' AND confidence IS NOT NULL")
            .fetch_one(&self.pool)
            .await?
            .try_get("avg")
            .ok();

        Ok(MemoryStats {
            total_memories: total_memories as usize,
            total_edges: total_edges as usize,
            memories_by_network: NetworkStats {
                world: world_count as usize,
                experience: experience_count as usize,
                opinion: opinion_count as usize,
                observation: observation_count as usize,
            },
            edges_by_type: EdgeTypeStats {
                temporal: temporal_count as usize,
                semantic: semantic_count as usize,
                entity: entity_count as usize,
                causal: causal_count as usize,
            },
            top_entities,
            recent_memories: recent_memories as usize,
            average_confidence: avg_confidence,
        })
    }

    /// Store file metadata after processing.
    pub async fn store_file_metadata(&self, file_metadata: &crate::files::FileMetadata) -> Result<()> {
        let file_type_str = match file_metadata.file_type {
            crate::files::FileType::PDF => "pdf",
            crate::files::FileType::Markdown => "markdown",
            crate::files::FileType::Text => "text",
            crate::files::FileType::Unknown => "unknown",
        };

        sqlx::query(
            r#"INSERT INTO files (id, filename, path, file_type, size_bytes, hash, processed_at, content_length, chunk_count)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
        )
        .bind(Uuid::parse_str(&file_metadata.id)?)
        .bind(&file_metadata.filename)
        .bind(file_metadata.path.to_string_lossy().as_ref())
        .bind(file_type_str)
        .bind(file_metadata.size_bytes as i64)
        .bind(&file_metadata.hash)
        .bind(file_metadata.processed_at)
        .bind(file_metadata.content_length as i32)
        .bind(file_metadata.chunk_count as i32)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get file metadata by hash.
    pub async fn get_file_by_hash(&self, hash: &str) -> Result<Option<crate::files::FileMetadata>> {
        let row = sqlx::query(
            "SELECT id, filename, path, file_type, size_bytes, hash, processed_at, content_length, chunk_count
             FROM files WHERE hash = $1"
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let file_type = match row.get::<String, _>("file_type").as_str() {
                    "pdf" => crate::files::FileType::PDF,
                    "markdown" => crate::files::FileType::Markdown,
                    "text" => crate::files::FileType::Text,
                    _ => crate::files::FileType::Unknown,
                };

                Ok(Some(crate::files::FileMetadata {
                    id: row.get::<Uuid, _>("id").to_string(),
                    filename: row.get("filename"),
                    path: row.get::<String, _>("path").into(),
                    file_type,
                    size_bytes: row.get("size_bytes"),
                    hash: row.get("hash"),
                    processed_at: row.get("processed_at"),
                    content_length: row.get("content_length"),
                    chunk_count: row.get("chunk_count"),
                }))
            }
            None => Ok(None),
        }
    }

    /// Get all processed files.
    pub async fn get_all_files(&self) -> Result<Vec<crate::files::FileMetadata>> {
        let rows = sqlx::query(
            "SELECT id, filename, path, file_type, size_bytes, hash, processed_at, content_length, chunk_count
             FROM files ORDER BY processed_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut files = Vec::new();
        for row in rows {
            let file_type = match row.get::<String, _>("file_type").as_str() {
                "pdf" => crate::files::FileType::PDF,
                "markdown" => crate::files::FileType::Markdown,
                "text" => crate::files::FileType::Text,
                _ => crate::files::FileType::Unknown,
            };

            files.push(crate::files::FileMetadata {
                id: row.get::<Uuid, _>("id").to_string(),
                filename: row.get("filename"),
                path: row.get::<String, _>("path").into(),
                file_type,
                size_bytes: row.get("size_bytes"),
                hash: row.get("hash"),
                processed_at: row.get("processed_at"),
                content_length: row.get("content_length"),
                chunk_count: row.get("chunk_count"),
            });
        }

        Ok(files)
    }

    /// Updates the confidence score of an opinion memory.
    ///
    /// Only affects rows where `network_type = 'opinion'`.
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

    /// Finds all opinion memories that share at least one entity with the
    /// given list. Used for opinion reinforcement during retention.
    pub async fn find_opinions_by_entities(&self, entities: &[String]) -> Result<Vec<MemoryUnit>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        for entity in entities {
            let pattern = format!("%\"{}\"%", entity.replace('%', "\\%").replace('_', "\\_"));
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

/// Formats a `Vec<f32>` as a PostgreSQL vector literal, e.g. `[0.1,0.2,0.3]`.
fn format_vector(v: &[f32]) -> String {
    let inner: Vec<String> = v.iter().map(|f| f.to_string()).collect();
    format!("[{}]", inner.join(","))
}

/// Parses a PostgreSQL vector literal back into a `Vec<f32>`.
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

/// Deserializes a `sqlx::postgres::PgRow` into a [`MemoryUnit`].
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
