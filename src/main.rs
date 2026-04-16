//! Hindsight — agentic memory architecture for AI agents.
//!
//! This binary runs an interactive REPL that accepts user messages, stores
//! extracted facts into a structured memory bank, recalls relevant context,
//! and generates preference-conditioned responses.
//!
//! See the module-level docs for each subsystem:
//!
//! - [`config`] — YAML-based configuration
//! - [`models`] — data structures (networks, edges, profile)
//! - [`llm`] — OpenAI-compatible HTTP client
//! - [`storage`] — PostgreSQL + pgvector persistence
//! - [`tempr`] — Retain & Recall pipeline
//! - [`cara`] — Reflect pipeline

mod api;
mod cara;
mod config;
mod llm;
mod models;
mod storage;
mod tempr;

use std::io::{self, Write};
use std::sync::Arc;
use std::env;

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

    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    let web_mode = args.len() > 1 && (args[1] == "--web" || args[1] == "--serve");
    let cli_mode = !web_mode || (args.len() > 2 && args[2] == "--cli");

    println!("Connecting to database at {}...", config.database.url);
    let storage = Arc::new(Storage::connect(&config.database.url).await?);
    storage.init_schema().await?;
    println!("Database schema initialized.");

    let llm = LLMClient::new(&config.llm);
    let embedding_dim = config.llm.embedding_dim;

    // Create a new storage connection for TEMPR (it takes ownership)
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

    let cara = CaraPipeline::new(profile, tempr);

    // Start web server if enabled or requested
    if web_mode || config.web.enabled {
        let web_host = config.web.host.clone();
        let web_port = config.web.port;

        let web_config = api::WebConfig {
            host: web_host.clone(),
            port: web_port,
            enabled: true,
        };

        let web_server = WebServer::new(web_config, storage.clone());

        if cli_mode {
            // Run both CLI and web server
            println!("🌐 Starting web server at http://{}:{}...", web_host, web_port);
            let web_handle = tokio::spawn(async move {
                if let Err(e) = web_server.run().await {
                    tracing::error!("Web server error: {}", e);
                }
            });

            println!("Hindsight agent ready. Type a message (or 'quit' to exit):\n");
            run_cli_repl(cara).await?;

            // Cancel web server when CLI exits
            web_handle.abort();
        } else {
            // Run web server only
            web_server.run().await?;
            return Ok(());
        }
    } else {
        println!("Hindsight agent ready. Type a message (or 'quit' to exit):\n");
        run_cli_repl(cara).await?;
    }

    println!("Goodbye!");
    Ok(())
}

/// Run the interactive CLI REPL.
async fn run_cli_repl(cara: CaraPipeline) -> Result<()> {
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            break;
        }

        if input.is_empty() {
            continue;
        }

        match cara.retain(input).await {
            Ok(memories) => {
                if !memories.is_empty() {
                    tracing::info!("Retained {} new memories", memories.len());
                }
            }
            Err(e) => tracing::error!("Retain error: {}", e),
        }

        match cara.reflect(input, 2000).await {
            Ok(response) => println!("\n{}\n", response),
            Err(e) => tracing::error!("Reflect error: {}", e),
        }
    }
    Ok(())
}
