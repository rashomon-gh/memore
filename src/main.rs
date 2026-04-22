//! Hindsight — agentic memory architecture for AI agents.
//!
//! This binary runs a web server that provides an interactive dashboard and API
//! for storing extracted facts into a structured memory bank, recalling relevant context,
//! and generating preference-conditioned responses.
//!
//! See the module-level docs for each subsystem:
//!
//! - [`config`] — YAML-based configuration
//! - [`models`] — data structures (networks, edges, profile)
//! - [`llm`] — OpenAI-compatible HTTP client
//! - [`storage`] — PostgreSQL + pgvector persistence
//! - [`tempr`] — Retain & Recall pipeline
//! - [`cara`] — Reflect pipeline
//! - [`api`] — Web server and REST API

mod api;
mod cara;
mod config;
mod llm;
mod models;
mod storage;
mod tempr;

use std::sync::Arc;

use anyhow::Result;
use cara::CaraPipeline;
use config::Config;
use llm::LLMClient;
use models::AgentProfile;
use storage::Storage;
use tempr::TemprPipeline;
use api::WebServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hindsight=info".into()),
        )
        .init();

    let config = Config::load()?;

    println!("Connecting to database at {}...", config.database.url);
    let storage = Arc::new(Storage::connect(&config.database.url).await?);
    storage.init_schema().await?;
    println!("Database schema initialized.");

    let llm = LLMClient::new(&config.llm);
    let embedding_dim = config.llm.embedding_dim;

    let storage_for_tempr = Storage::connect(&config.database.url).await?;
    let tempr = TemprPipeline::new(llm, storage_for_tempr, embedding_dim);

    let profile = AgentProfile {
        name: "Hindsight".into(),
        background: "I am an AI agent with a structured long-term memory system. I can retain, recall, and reflect on information across conversations.".into(),
        skepticism: 3,
        literalism: 2,
        empathy: 4,
        bias_strength: 0.5,
    };

    let cara = Arc::new(CaraPipeline::new(profile, tempr));

    let web_host = config.web.host.clone();
    let web_port = config.web.port;

    let web_config = api::WebConfig {
        host: web_host.clone(),
        port: web_port,
    };

    let web_server = WebServer::new(web_config, storage, cara);

    println!("🌐 Starting web server at http://{}:{}...", web_host, web_port);

    web_server.run().await?;

    println!("Goodbye!");
    Ok(())
}
