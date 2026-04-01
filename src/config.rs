use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub llm: LLMConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LLMConfig {
    pub base_url: String,
    pub api_key: String,
    pub chat_model: String,
    pub embed_model: String,
    pub embedding_dim: usize,
}

impl Config {
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
