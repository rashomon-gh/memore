//! OpenAI-compatible LLM client for chat completions and text embeddings.
//!
//! Communicates with any server that implements the `/v1/chat/completions`
//! and `/v1/embeddings` endpoints (e.g. LM Studio, Ollama, vLLM).

use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, error, instrument};

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
    max_tokens: u64,
}

impl LLMClient {
    /// Creates a new client from the given [`LLMConfig`].
    pub fn new(config: &LLMConfig) -> Self {
        info!(
            base_url = %config.base_url,
            embed_base_url = %config.embed_base_url.as_ref().unwrap_or(&config.base_url),
            chat_model = %config.chat_model,
            embed_model = %config.embed_model,
            max_tokens = config.max_tokens,
            "Creating LLM client"
        );
        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(60))
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        let embed_base_url = config
            .embed_base_url
            .as_ref()
            .unwrap_or(&config.base_url)
            .trim_end_matches('/')
            .to_string();

        debug!("LLM client created successfully");
        Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            embed_base_url,
            api_key: config.api_key.clone(),
            chat_model: config.chat_model.clone(),
            embed_model: config.embed_model.clone(),
            max_tokens: config.max_tokens,
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
    #[instrument(skip(self, messages))]
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        temperature: Option<f64>,
        max_tokens: Option<u64>,
    ) -> Result<String> {
        let message_count = messages.len();
        let user_messages: usize = messages.iter().filter(|m| m.role == "user").count();
        debug!(
            message_count = message_count,
            user_messages = user_messages,
            temperature = ?temperature,
            max_tokens = ?max_tokens,
            model = %self.chat_model,
            "Sending chat completion request"
        );
        let request = ChatRequest {
            model: self.chat_model.clone(),
            messages,
            temperature,
            max_tokens,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        debug!(url = %url, "Sending request to LLM API");
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %body,
                "LLM chat request failed"
            );
            return Err(anyhow!("LLM chat request failed ({}): {}", status, body));
        }

        let chat_response: ChatResponse = response.json().await?;

        let reply = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| {
                error!("No response choices from LLM");
                anyhow!("No response choices from LLM")
            })?;

        info!(
            reply_length = reply.len(),
            "Chat completion successful"
        );
        Ok(reply)
    }

    /// Generates embeddings for a batch of texts.
    ///
    /// Returns one `Vec<f32>` per input string, in the same order.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the server returns a
    /// non-success status.
    #[instrument(skip(self, texts))]
    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let text_count = texts.len();
        let total_chars: usize = texts.iter().map(|t| t.len()).sum();
        debug!(
            text_count = text_count,
            total_chars = total_chars,
            model = %self.embed_model,
            "Generating embeddings"
        );
        let request = EmbedRequest {
            model: self.embed_model.clone(),
            input: texts,
        };

        let url = format!("{}/v1/embeddings", self.embed_base_url);
        debug!(url = %url, "Sending request to embeddings API");
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %body,
                "LLM embed request failed"
            );
            return Err(anyhow!("LLM embed request failed ({}): {}", status, body));
        }

        let embed_response: EmbedResponse = response.json().await?;

        let embeddings: Vec<Vec<f32>> = embed_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect();

        info!(
            embeddings_count = embeddings.len(),
            embedding_dim = embeddings.first().map(|e| e.len()).unwrap_or(0),
            "Embeddings generated successfully"
        );
        Ok(embeddings)
    }

    /// Convenience wrapper around [`embed`](Self::embed) for a single text.
    pub async fn embed_single(&self, text: String) -> Result<Vec<f32>> {
        let embeddings = self.embed(vec![text]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No embedding returned"))
    }

    /// Returns the max_tokens limit for chat completions.
    pub fn max_tokens(&self) -> u64 {
        self.max_tokens
    }
}
