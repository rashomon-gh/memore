## **Primary Objective**

You are an expert Rust developer tasked with implementing the "Memor/e" agentic memory architecture. Your goal is to build a high-performance, structured memory system that enables AI agents to retain, recall, and reflect on long-term interactions, moving beyond standard stateless question answering.

## ---

**Technical Stack & Constraints**

* **Primary Language:** Rust (Use stable, idiomatic Rust with asynchronous paradigms, e.g., tokio).
* **LLM Inference:** Must interface via an OpenAI-compatible REST API endpoint to support local or self-hosted models like Ollama or vLLM.

* **Data Storage:** Implement a storage layer capable of handling vector embeddings, full-text search, and relational/graph data (Recommendation: PostgreSQL with pgvector for semantic search and a GIN index for BM25 ranking ).

* **Configuration:** Support loading configuration via a custom file path passed as a CLI argument, defaulting to `config.yaml` if not provided.

* **Documentation:** You must generate and maintain a README.md file. This file must be strictly concise, providing only the gist of the project and instructions on how to run it. **Do not** include lengthy descriptions, underlying theory, or directory structures.

## ---

**Architecture Requirements**

### **1\. The Memory Networks (Data Structures)**

You must implement a memory bank organized into four distinct logical networks:

* **World Network (W):** Stores objective facts about the external world.

* **Experience Network (B):** Stores biographical information about the agent itself, written in the first person.

* **Opinion Network (O):** Stores subjective judgments as a tuple containing the text, a confidence score between 0.0 and 1.0, and a timestamp.

* **Observation Network (S):** Stores preference-neutral, synthesized summaries of entities.

### **2\. The Core Operations (Logic Layer)**

Implement the following three primary operations to interact with the memory networks:

* **Retain (TEMPR Component):**
    * Parse conversational inputs to extract self-contained, narrative facts.

    * Classify each extracted fact into one of the four networks.

    * Extract entities and establish graph links (temporal, semantic, entity, and causal) between memories.

    * Implement an opinion reinforcement mechanism to dynamically adjust confidence scores when new, related facts are ingested.

* **Recall (TEMPR Component):**
    * Implement a retrieval interface that accepts a query and a strict token budget.

    * Execute a multi-strategy search in parallel: Semantic (vector similarity), Keyword (BM25), Graph traversal (spreading activation), and Temporal filtering.

    * Merge the results from all four channels using Reciprocal Rank Fusion (RRF) and apply token budget filtering to fit the context window.

* **Reflect (CARA Component):**
    * Define an agent profile containing a name, background, and disposition parameters: Skepticism, Literalism, Empathy (values 1-5), and Bias Strength (0.0-1.0).

    * Combine retrieved memory contexts with the behavioral profile to generate preference-conditioned responses via the LLM API.

    * Extract any newly formed opinions from the LLM's response and save them to the Opinion Network.

## ---

**Implementation Phasing**

1. **Bootstrap & API Clients:** Set up the Rust workspace, error handling, and the HTTP client for the OpenAI-compatible LLM endpoint.
2. **Storage Engine:** Define the Rust structs representing the memory units  and set up the database schema for the four networks and edge relationships.

3. **TEMPR Pipeline:** Implement the Retain and Recall logic, ensuring concurrent execution of the four retrieval strategies.
4. **CARA Pipeline:** Implement the Reflect logic, system prompt formatting, and opinion extraction.
5. **Documentation:** Generate the required concise README.md.
