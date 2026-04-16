//! File processing and integration with TEMPR pipeline.

use anyhow::Result;
use chrono::Utc;
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use uuid::Uuid;

use crate::files::extractor::{extract_content, detect_file_type};
use crate::files::{FileType, FileMetadata, ProcessingResult, ExtractedContent};
use crate::llm::LLMClient;
use crate::storage::Storage;
use crate::models::NetworkType;

/// File processor that integrates with the TEMPR pipeline.
pub struct FileProcessor {
    storage: std::sync::Arc<Storage>,
    llm: std::sync::Arc<LLMClient>,
    embedding_dim: usize,
    chunk_size: usize,
    chunk_overlap: usize,
}

impl FileProcessor {
    /// Create a new file processor.
    pub fn new(storage: std::sync::Arc<Storage>, llm: std::sync::Arc<LLMClient>, embedding_dim: usize) -> Self {
        Self {
            storage,
            llm,
            embedding_dim,
            chunk_size: 2000, // Characters per chunk
            chunk_overlap: 200, // Overlap between chunks
        }
    }

    /// Process a single file and extract memories.
    pub async fn process_file(&self, file_path: PathBuf) -> Result<ProcessingResult> {
        let start_time = std::time::Instant::now();

        // Detect file type
        let file_type = detect_file_type(&file_path);
        if file_type == FileType::Unknown {
            return Err(anyhow::anyhow!("Unknown file type: {:?}", file_path));
        }

        // Extract content
        let extracted = extract_content(&file_path, &file_type)?;

        // Calculate file hash
        let file_content = std::fs::read(&file_path)?;
        let hash = format!("{:x}", Sha256::digest(&file_content));

        // Check if file was already processed
        if let Some(existing_metadata) = self.storage.get_file_by_hash(&hash).await? {
            return Ok(ProcessingResult {
                file_metadata: existing_metadata,
                content_chunks: Vec::new(),
                total_memories_created: 0,
                processing_time_ms: start_time.elapsed().as_millis() as u64,
            });
        }

        // Chunk content
        let chunks = self.chunk_content(&extracted.text);

        // Process each chunk through TEMPR
        let mut total_memories = 0;
        for (chunk_num, chunk) in chunks.iter().enumerate() {
            let memories = self.process_chunk(
                chunk,
                &file_path,
                &extracted,
                chunk_num,
                chunks.len()
            ).await?;

            total_memories += memories.len();
        }

        // Store file metadata
        let file_metadata = FileMetadata {
            id: Uuid::new_v4().to_string(),
            filename: file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            path: file_path.clone(),
            file_type,
            size_bytes: file_content.len() as i64,
            hash,
            processed_at: Utc::now(),
            content_length: extracted.text.len() as i32,
            chunk_count: chunks.len() as i32,
        };

        self.storage.store_file_metadata(&file_metadata).await?;

        Ok(ProcessingResult {
            file_metadata,
            content_chunks: chunks,
            total_memories_created: total_memories,
            processing_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// Process a chunk of content and extract memories.
    async fn process_chunk(
        &self,
        chunk: &str,
        file_path: &PathBuf,
        extracted: &ExtractedContent,
        chunk_num: usize,
        total_chunks: usize,
    ) -> Result<Vec<Uuid>> {
        // Create a prompt for the LLM to extract facts from the chunk
        let source_context = format!(
            "From file: {} (chunk {}/{})",
            file_path.display(),
            chunk_num + 1,
            total_chunks
        );

        let prompt = format!(
            "Extract important facts, information, and knowledge from the following text content. \
             Consider the source context: '{}'\n\nText:\n{}",
            source_context, chunk
        );

        // Use the LLM to extract structured facts
        let messages = vec![
            crate::llm::ChatMessage {
                role: "system".to_string(),
                content: "You are an expert at extracting structured knowledge from text. \
                          Extract facts and classify them into appropriate networks: \
                          - World: objective facts about the external world \
                          - Experience: biographical information or first-person accounts \
                          - Opinion: subjective judgments (include confidence scores) \
                          - Observation: neutral summaries of entities or topics \
                          Extract entities and relationships where appropriate.".to_string(),
            },
            crate::llm::ChatMessage {
                role: "user".to_string(),
                content: prompt,
            },
        ];

        let response = self.llm.chat_completion(messages, None, Some(4000)).await?;

        // Parse the response and create memories
        let memory_ids = self.parse_and_store_memories(&response, file_path, extracted).await?;

        Ok(memory_ids)
    }

    /// Parse LLM response and store memories in the database.
    async fn parse_and_store_memories(
        &self,
        response: &str,
        file_path: &PathBuf,
        _extracted: &ExtractedContent,
    ) -> Result<Vec<Uuid>> {
        // This is a simplified version - in production, you'd want more robust parsing
        // For now, we'll split by common delimiters and create memories

        let mut memory_ids = Vec::new();

        // Split response into potential fact statements
        let facts: Vec<&str> = response
            .split(&['.', '\n', '\r'][..])
            .map(|s| s.trim())
            .filter(|s| s.len() > 20) // Filter out short fragments
            .collect();

        for fact in facts {
            if fact.is_empty() {
                continue;
            }

            // Generate embedding
            let embedding = self.llm.embed_single(fact.to_string()).await?;

            // Determine network type (simplified logic)
            let network = self.classify_network(fact);

            // Extract entities (simplified)
            let entities = self.extract_entities(fact);

            // Store memory
            let memory_id = Uuid::new_v4();
            self.storage.store_memory(
                memory_id,
                network,
                &format!("{} (from: {})", fact, file_path.display()),
                &embedding,
                &entities,
                None,
            ).await?;

            memory_ids.push(memory_id);
        }

        Ok(memory_ids)
    }

    /// Classify content into appropriate network type.
    fn classify_network(&self, content: &str) -> NetworkType {
        let content_lower = content.to_lowercase();

        // Simple heuristics for classification
        if content_lower.contains("i think") || content_lower.contains("i believe") ||
           content_lower.contains("in my opinion") || content_lower.contains("seems like") {
            NetworkType::Opinion
        } else if content_lower.contains("i ") || content_lower.contains("my ") {
            NetworkType::Experience
        } else if content_lower.contains("is defined as") || content_lower.contains("refers to") {
            NetworkType::Observation
        } else {
            NetworkType::World
        }
    }

    /// Extract entities from content (simplified).
    fn extract_entities(&self, content: &str) -> Vec<String> {
        // This is a very basic implementation
        // In production, you'd use NLP libraries or the LLM

        let mut entities = Vec::new();

        // Look for capitalized words (potential proper nouns)
        let words: Vec<&str> = content.split_whitespace().collect();
        for i in 0..words.len().saturating_sub(1) {
            let word = words[i];
            if word.len() > 2 && word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                // Check if it's likely a proper noun (capitalized and not at sentence start)
                if i == 0 || (words.get(i.saturating_sub(1)).map(|w| w.ends_with('.')).unwrap_or(false)) {
                    continue; // Skip sentence starts
                }
                entities.push(word.to_string());
            }
        }

        entities.dedup();
        entities.truncate(10); // Limit to 10 entities
        entities
    }

    /// Chunk content into manageable pieces.
    fn chunk_content(&self, content: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = content.chars().collect();
        let mut position = 0;

        while position < chars.len() {
            let end = (position + self.chunk_size).min(chars.len());

            // Try to break at a sentence boundary
            let chunk_end = if end < chars.len() {
                self.find_sentence_boundary(&chars, position, end)
            } else {
                end
            };

            let chunk: String = chars[position..chunk_end].iter().collect();
            chunks.push(chunk.trim().to_string());

            // Move to next chunk with overlap
            position = chunk_end.saturating_sub(self.chunk_overlap);
        }

        chunks
    }

    /// Find a good sentence boundary for chunking.
    fn find_sentence_boundary(&self, chars: &[char], start: usize, end: usize) -> usize {
        // Look for sentence-ending punctuation
        for i in (start..end).rev() {
            let ch = chars[i];
            if ch == '.' || ch == '!' || ch == '?' {
                // Make sure we have some space after the punctuation
                if i + 1 < chars.len() && chars[i + 1].is_whitespace() {
                    return i + 1;
                }
            }
        }

        // If no sentence boundary found, try word boundary
        for i in (start..end).rev() {
            if chars[i].is_whitespace() {
                return i + 1;
            }
        }

        // Last resort: just use the end position
        end
    }
}
