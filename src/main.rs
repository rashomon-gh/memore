mod cara;
mod config;
mod llm;
mod models;
mod storage;
mod tempr;

use std::io::{self, Write};

use anyhow::Result;
use cara::CaraPipeline;
use config::Config;
use llm::LLMClient;
use models::AgentProfile;
use storage::Storage;
use tempr::TemprPipeline;

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
    let storage = Storage::connect(&config.database.url).await?;
    storage.init_schema().await?;
    println!("Database schema initialized.");

    let llm = LLMClient::new(&config.llm);
    let embedding_dim = config.llm.embedding_dim;
    let tempr = TemprPipeline::new(llm, storage, embedding_dim);

    let profile = AgentProfile {
        name: "Hindsight".into(),
        background: "I am an AI agent with a structured long-term memory system. I can retain, recall, and reflect on information across conversations.".into(),
        skepticism: 3,
        literalism: 2,
        empathy: 4,
        bias_strength: 0.5,
    };

    let cara = CaraPipeline::new(profile, tempr);

    println!("Hindsight agent ready. Type a message (or 'quit' to exit):\n");

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

    println!("Goodbye!");
    Ok(())
}
