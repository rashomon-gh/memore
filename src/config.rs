#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub llm_base_url: String,
    pub api_key: String,
    pub chat_model: String,
    pub embed_model: String,
    pub embedding_dim: usize,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://hindsight:hindsight@localhost:5432/hindsight".into()
            }),
            llm_base_url: std::env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234".into()),
            api_key: std::env::var("LLM_API_KEY").unwrap_or_else(|_| "local".into()),
            chat_model: std::env::var("CHAT_MODEL").unwrap_or_else(|_| "google/gemma-3-27b".into()),
            embed_model: std::env::var("EMBED_MODEL")
                .unwrap_or_else(|_| "nomic-ai/nomic-embed-text-v1.5-GGUF".into()),
            embedding_dim: std::env::var("EMBEDDING_DIM")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(768),
        }
    }
}
