# Hindsight

Agentic memory architecture for AI agents — retain, recall, and reflect on long-term interactions.

## Requirements

- Rust 1.85+
- PostgreSQL with [pgvector](https://github.com/pgvector/pgvector) extension
- OpenAI-compatible LLM endpoint (e.g., LM Studio, Ollama, vLLM)

## Configuration

Set environment variables (defaults shown):

```bash
export DATABASE_URL="postgres://hindsight:hindsight@localhost:5432/hindsight"
export LLM_BASE_URL="http://127.0.0.1:1234"
export LLM_API_KEY="local"
export CHAT_MODEL="google/gemma-3-27b"
export EMBED_MODEL="nomic-ai/nomic-embed-text-v1.5-GGUF"
export EMBEDDING_DIM=768
```

## Run

```bash
cargo run
```
