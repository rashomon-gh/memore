//! OpenAI-compatible LLM client for chat completions and text embeddings.
//!
//! Communicates with any server that implements the `/v1/chat/completions`
//! and `/v1/embeddings` endpoints (e.g. LM Studio, Ollama, vLLM).

use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::LLMConfig;

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role: `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageContent,
}

#[derive(Debug, Deserialize)]
struct ChatMessageContent {
    content: String,
}

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Debug, Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

/// HTTP client for an OpenAI-compatible LLM API.
pub struct LLMClient {
    client: Client,
    base_url: String,
    embed_base_url: String,
    api_key: String,
    chat_model: String,
    embed_model: String,
}

impl LLMClient {
    /// Creates a new client from the given [`LLMConfig`].
    pub fn new(config: &LLMConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("failed to build HTTP client");

        let embed_base_url = config
            .embed_base_url
            .as_ref()
            .unwrap_or(&config.base_url)
            .trim_end_matches('/')
            .to_string();

        Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            embed_base_url,
            api_key: config.api_key.clone(),
            chat_model: config.chat_model.clone(),
            embed_model: config.embed_model.clone(),
        }
    }

    /// Sends a chat-completion request and returns the assistant's reply.
    ///
    /// The `temperature` and `max_tokens` parameters are forwarded to the API
    /// when provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the server returns a
    /// non-success status, or the response contains no choices.
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
    ) -> Result<String> {
        let request = ChatRequest {
            model: self.chat_model.clone(),
            messages,
            temperature,
            max_tokens,
        };

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("LLM chat request failed ({}): {}", status, body));
        }

        let chat_response: ChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("No response choices from LLM"))
    }

    /// Generates embeddings for a batch of texts.
    ///
    /// Returns one `Vec<f32>` per input string, in the same order.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the server returns a
    /// non-success status.
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let request = EmbedRequest {
            model: self.embed_model.clone(),
            input: texts,
        };

        let response = self
            .client
            .post(format!("{}/v1/embeddings", self.embed_base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("LLM embed request failed ({}): {}", status, body));
        }

        let embed_response: EmbedResponse = response.json().await?;

        Ok(embed_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect())
    }

    /// Convenience wrapper around [`embed`](Self::embed) for a single text.
    pub async fn embed_single(&self, text: String) -> Result<Vec<f32>> {
        let embeddings = self.embed(vec![text]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No embedding returned"))
    }
}
