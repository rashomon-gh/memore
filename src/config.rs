//! Application configuration loaded from `config.yaml`.
//!
//! Uses the [`config`] crate to deserialize a YAML file into strongly-typed
//! configuration structs. The config file is expected at the project root.
//!
//! # Example `config.yaml`
//!
//! ```yaml
//! database:
//!   url: "postgres://hindsight:hindsight@localhost:5432/hindsight"
//! llm:
//!   base_url: "http://127.0.0.1:1234"
//!   embed_base_url: "http://localhost:1234"
//!   api_key: "local"
//!   chat_model: "google/gemma-3-27b"
//!   embed_model: "nomic-ai/nomic-embed-text-v1.5-GGUF"
//!   embedding_dim: 768
//! web:
//!   enabled: true
//!   host: "127.0.0.1"
//!   port: 8080
//! ```

use anyhow::{Context, Result};
use serde::Deserialize;

/// Top-level configuration containing database, LLM, and web settings.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// PostgreSQL connection settings.
    pub database: DatabaseConfig,
    /// LLM endpoint settings.
    pub llm: LLMConfig,
    /// Web server configuration.
    #[serde(default)]
    pub web: WebConfig,
}

/// PostgreSQL connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Connection string, e.g. `postgres://user:pass@host:port/db`.
    pub url: String,
}

/// OpenAI-compatible LLM endpoint configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LLMConfig {
    /// Base URL of the API for chat completions (e.g. `http://127.0.0.1:1234`).
    pub base_url: String,
    /// Optional separate base URL for embeddings (e.g. `http://localhost:1234`).
    /// If not provided, falls back to `base_url`.
    #[serde(default)]
    pub embed_base_url: Option<String>,
    /// Bearer token sent in the `Authorization` header.
    pub api_key: String,
    /// Model identifier used for chat completions.
    pub chat_model: String,
    /// Model identifier used for text embeddings.
    pub embed_model: String,
    /// Dimensionality of the embedding vectors produced by `embed_model`.
    pub embedding_dim: usize,
}

/// Web server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    /// Whether to enable the web server.
    #[serde(default)]
    pub enabled: bool,
    /// Host to bind the web server to.
    #[serde(default = "default_host")]
    pub host: String,
    /// Port to bind the web server to.
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

impl Config {
    /// Loads configuration from `config.yaml` in the current directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is missing or cannot be parsed.
    pub fn load() -> Result<Self> {
        let settings = config::Config::builder()
            .add_source(config::File::with_name("config.yaml"))
            .build()
            .context("Failed to load config.yaml")?;

        settings
            .try_deserialize()
            .context("Failed to parse config.yaml")
    }
}
