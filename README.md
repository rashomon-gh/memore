# Memor/e

[![Build](https://github.com/rashomon-gh/hindsight/actions/workflows/build.yml/badge.svg)](https://github.com/rashomon-gh/hindsight/actions/workflows/build.yml/badge.svg)


An LLM agent implementation with self evolving memory, based on 
[Hindsight is 20/20: Agent Memory that Retains, Recalls, and Reflects](http://arxiv.org/abs/2512.12818).

## Features

- **Four Memory Networks**: World, Experience, Opinion, and Observation networks for structured knowledge storage
- **Semantic Search**: HNSW-indexed vector embeddings for fast similarity search
- **Knowledge Graph**: Temporal, semantic, entity, and causal relationships between memories
- **Interactive Web Dashboard**: Real-time visualization of memory networks, relationships, and analytics
- **Multi-Strategy Retrieval**: Parallel semantic, keyword, temporal, and graph traversal search
- **Agent Profile System**: Configurable behavioral parameters (skepticism, literalism, empathy)

## Requirements

- Rust 1.85+
- Docker (for PostgreSQL)
- OpenAI-compatible LLM endpoint (e.g., LM Studio, Ollama, vLLM)

## Run

1. Start PostgreSQL with pgvector

```bash
docker compose up -d
```

2. Copy and edit the config (defaults should work for local development)

```bash
cp config.yaml.example config.yaml
```

3. Build and run the web server

```bash
cargo run
```

Access the dashboard at `http://127.0.0.1:8080/`

## Web Dashboard

The interactive web dashboard provides:

- **🕸️ Network Graph**: Interactive visualization of memory relationships with Cytoscape.js
- **🔍 Search & Filter**: Full-text search with network and entity filtering
- **📊 Analytics**: Memory distribution charts, entity frequencies, and statistics
- **🔬 Memory Inspector**: Detailed memory information with related memories

### API Endpoints

The dashboard exposes REST API endpoints:

- `GET /api/memories` - List/search memories with pagination
- `GET /api/memories/:id` - Get single memory with neighbors
- `GET /api/graph` - Export graph data for visualization
- `GET /api/entities` - List all unique entities
- `GET /api/stats` - Analytics statistics
- `GET /api/networks/:type` - Filter memories by network type

Example API usage:

```bash
# Get all memories
curl http://localhost:8080/api/memories

# Search memories
curl http://localhost:8080/api/memories?search=skiing

# Get graph data
curl http://localhost:8080/api/graph

# Get statistics
curl http://localhost:8080/api/stats
```

## Configuration

Configuration is loaded from `config.yaml` in the project root. See `config.yaml.example` for all options.

Key configuration options:

```yaml
database:
  url: "postgres://hindsight:hindsight@localhost:5432/hindsight"

llm:
  base_url: "https://your-llm-endpoint.com"
  embed_base_url: "http://localhost:1234"  # Separate endpoint for embeddings
  api_key: "your-api-key"
  chat_model: "your-chat-model"
  embed_model: "your-embed-model"
  embedding_dim: 768

web:
  host: "127.0.0.1"
  port: 8080
```

### Example Configuration - LM Studio

```yaml
database:
  url: "postgres://hindsight:hindsight@localhost:5432/hindsight"

llm:
  base_url: "http://127.0.0.1:1234"
  embed_base_url: "http://localhost:1234"
  api_key: "Bearer token"
  chat_model: "google/gemma-4-26b-a4b"
  embed_model: "nomic-ai/nomic-embed-text-v1.5-GGUF"
  embedding_dim: 768
  max_tokens: 16384

web:
  host: "127.0.0.1"
  port: 8080
```

## Architecture

### TEMPR Pipeline (Temporal-Entity Memory Processing & Retrieval)

**Retain Operation:**

- LLM parses conversation to extract structured facts
- Classification into one of four networks
- Entity extraction and embedding generation
- Storage in PostgreSQL with vector indexes
- Graph edge creation between related facts
- Opinion reinforcement for related entities

**Recall Operation:**

- Parallel execution of 4 retrieval strategies
- Spreading activation graph traversal (3 hops)
- Reciprocal Rank Fusion (RRF) for result merging
- Token budget management for context limits

### Memory Networks

- **World**: Objective facts about the external world
- **Experience**: Biographical information about the agent (first-person)
- **Opinion**: Subjective judgments with confidence scores (0.0–1.0)
- **Observation**: Preference-neutral synthesized summaries of entities

### Knowledge Graph

Memory units are connected by four types of relationships:

- **Temporal**: Sequential/time-based relationships
- **Semantic**: Meaning-based similarity relationships
- **Entity**: Shared-entity reference relationships
- **Causal**: Cause-and-effect relationships
