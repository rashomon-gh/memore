//! File processing module for PDF and Markdown documents.
//!
//! Handles text extraction, content chunking, and metadata generation
//! for various document types to be ingested into the memory system.

pub mod extractor;
pub mod processor;

// pub mod watcher; // TODO: Implement file watching module

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Metadata about a processed file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: String,
    pub filename: String,
    pub path: PathBuf,
    pub file_type: FileType,
    pub size_bytes: i64,
    pub hash: String,
    pub processed_at: DateTime<Utc>,
    pub content_length: i32,
    pub chunk_count: i32,
}

/// Supported file types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    PDF,
    Markdown,
    Text,
    Unknown,
}

/// Extracted content from a document.
#[derive(Debug, Clone)]
pub struct ExtractedContent {
    pub text: String,
    pub metadata: ContentMetadata,
    pub code_blocks: Vec<CodeBlock>,
}

/// Metadata about extracted content.
#[derive(Debug, Clone)]
pub struct ContentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_date: Option<DateTime<Utc>>,
    pub language: Option<String>,
}

/// A code block detected in the content.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub language: Option<String>,
    pub code: String,
    pub line_start: usize,
    pub line_end: usize,
}

/// Processing result for a single file.
#[derive(Debug, Clone)]
pub struct ProcessingResult {
    pub file_metadata: FileMetadata,
    pub content_chunks: Vec<String>,
    pub total_memories_created: usize,
    pub processing_time_ms: u64,
}
