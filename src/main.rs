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
use tokio::io::AsyncBufReadExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hindsight=info".into()),
        )
        .init();

    let config = Config::load()?;

    let args: Vec<String> = env::args().collect();
    let web_mode = args.len() > 1 && (args[1] == "--web" || args[1] == "--serve");
    let cli_mode = !web_mode || (args.len() > 2 && args[2] == "--cli");

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

    let cara = CaraPipeline::new(profile, tempr);

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
            println!("🌐 Starting web server at http://{}:{}...", web_host, web_port);
            println!("Hindsight agent ready. Type a message (or 'quit' to exit):\n");

            tokio::select! {
                result = web_server.run() => {
                    result?;
                }
                result = run_cli_repl(cara) => {
                    result?;
                }
            }
        } else {
            web_server.run().await?;
        }
    } else {
        println!("Hindsight agent ready. Type a message (or 'quit' to exit):\n");
        run_cli_repl(cara).await?;
    }

    println!("Goodbye!");
    Ok(())
}

async fn run_cli_repl(cara: CaraPipeline) -> Result<()> {
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        print!("> ");
        io::stdout().flush()?;

        let input: String = match lines.next_line().await? {
            Some(line) => line,
            None => break,
        };

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
