# Hindsight

An unofficial implementation of 
[Hindsight is 20/20: Building Agent Memory that Retains, Recalls, and Reflects](http://arxiv.org/abs/2512.12818) 
in Rust.
This project implements the agentic memory management architecture proposed in the paper.


## Requirements

- Rust 1.85+
- Docker (for PostgreSQL)
- OpenAI-compatible LLM endpoint (e.g., LM Studio, Ollama, vLLM)

## Run

1. Start PostgreSQL with pgvector:

```bash
docker compose up -d
```

2. Copy and edit the config (defaults should work for local development):

```bash
cp config.yaml.example config.yaml
```

3. Build and run:

```bash
cargo run
```

Configuration is loaded from `config.yaml` in the project root. See `config.yaml.example` for all options.
