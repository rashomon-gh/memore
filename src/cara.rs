//! CARA pipeline — **Reflect** operation.
//!
//! Combines the TEMPR recall results with an [`AgentProfile`] to generate a
//! preference-conditioned LLM response. Any new opinions embedded in the
//! response as `<opinion confidence="…">…</opinion>` tags are extracted and
//! automatically stored in the Opinion network.

use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

use crate::llm::ChatMessage;
use crate::models::*;
use crate::tempr::TemprPipeline;

/// The CARA (Context-Aware Reflective Agent) pipeline.
///
/// Wraps a [`TemprPipeline`] and an [`AgentProfile`] to provide the
/// high-level [`retain`](Self::retain) and [`reflect`](Self::reflect) cycle.
pub struct CaraPipeline {
    profile: AgentProfile,
    tempr: TemprPipeline,
}

impl CaraPipeline {
    /// Creates a new CARA pipeline with the given profile and TEMPR backend.
    pub fn new(profile: AgentProfile, tempr: TemprPipeline) -> Self {
        Self { profile, tempr }
    }

    /// Delegates to [`TemprPipeline::retain`].
    pub async fn retain(&self, conversation: &str, chat_id: Option<Uuid>) -> Result<Vec<MemoryUnit>> {
        self.tempr.retain(conversation, chat_id).await
    }

    /// Recalls relevant memories, formats a system prompt with the agent's
    /// disposition profile, queries the LLM, then extracts and persists any
    /// new opinions found in the response.
    ///
    /// `token_budget` controls how many tokens of recalled context are
    /// injected into the prompt.
    pub async fn reflect(&self, user_message: &str, token_budget: usize, chat_id: Option<Uuid>) -> Result<(String, Vec<MemoryUnit>)> {
        let recalled = self.tempr.recall(user_message, token_budget).await?;

        let memory_context = if recalled.is_empty() {
            "No relevant memories found.".to_string()
        } else {
            recalled
                .iter()
                .map(|sm| {
                    let network_tag = match sm.memory.network {
                        NetworkType::World => "[World]",
                        NetworkType::Experience => "[Experience]",
                        NetworkType::Opinion => "[Opinion]",
                        NetworkType::Observation => "[Observation]",
                    };
                    let conf = sm
                        .memory
                        .confidence
                        .map(|c| format!(" (confidence: {:.2})", c))
                        .unwrap_or_default();
                    format!(
                        "{} {}{} — relevance: {:.3}",
                        network_tag, sm.memory.content, conf, sm.score
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let system_prompt = format!(
            r#"You are {name}. {background}

Disposition profile:
- Skepticism: {skepticism}/5 (how much you question claims)
- Literalism: {literalism}/5 (how literally you interpret things)
- Empathy: {empathy}/5 (how much you consider others' feelings)
- Bias Strength: {bias_strength:.2} (how strongly your existing opinions influence responses)

Your relevant memories:
{memory_context}

Respond naturally to the user. You may form new opinions based on the conversation and your memories. If you form or update any opinions, express them inside XML tags like:
<opinion confidence="0.7">opinion text here</opinion>
You may include multiple opinion tags. Do not mention the XML tags in your visible response style—just embed them."#,
            name = self.profile.name,
            background = self.profile.background,
            skepticism = self.profile.skepticism,
            literalism = self.profile.literalism,
            empathy = self.profile.empathy,
            bias_strength = self.profile.bias_strength,
            memory_context = memory_context,
        );

        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt,
            },
            ChatMessage {
                role: "user".into(),
                content: user_message.into(),
            },
        ];

        let response = self
            .tempr
            .llm()
            .chat_completion(messages, Some(0.7), Some(2048))
            .await?;

        let (clean_response, new_opinions) = extract_opinions(&response);

        let mut stored_opinions = Vec::new();
        for (opinion_text, confidence) in &new_opinions {
            let embedding = self
                .tempr
                .llm()
                .embed_single(opinion_text.clone())
                .await
                .unwrap_or_default();
            let id = Uuid::new_v4();
            self.tempr
                .storage()
                .store_memory(
                    id,
                    NetworkType::Opinion,
                    opinion_text,
                    &embedding,
                    &[],
                    Some(*confidence),
                    chat_id,
                )
                .await?;
            tracing::info!(
                "Stored new opinion: {} (confidence: {:.2})",
                opinion_text,
                confidence
            );
            stored_opinions.push(MemoryUnit {
                id,
                network: NetworkType::Opinion,
                content: opinion_text.clone(),
                embedding: vec![],
                entities: vec![],
                confidence: Some(*confidence),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }

        Ok((clean_response, stored_opinions))
    }
}

/// Extracts `<opinion confidence="…">…</opinion>` tags from an LLM response.
///
/// Returns the response text with all tags removed and a list of
/// `(text, confidence)` tuples.
fn extract_opinions(text: &str) -> (String, Vec<(String, f32)>) {
    let mut opinions = Vec::new();
    let mut clean = text.to_string();

    let tag_start = "<opinion";
    let tag_end = "</opinion>";

    while let Some(start_idx) = clean.find(tag_start) {
        let content_start = match clean[start_idx..].find('>') {
            Some(offset) => start_idx + offset + 1,
            None => break,
        };

        let end_idx = match clean[content_start..].find(tag_end) {
            Some(offset) => content_start + offset,
            None => break,
        };

        let opening_tag = &clean[start_idx..content_start];
        let opinion_text = clean[content_start..end_idx].trim().to_string();

        if let Some(conf) = extract_confidence(opening_tag)
            && !opinion_text.is_empty()
        {
            opinions.push((opinion_text, conf.clamp(0.0, 1.0)));
        }

        let full_end = end_idx + tag_end.len();
        clean = format!("{}{}", &clean[..start_idx], &clean[full_end..]);
    }

    (clean.trim().to_string(), opinions)
}

/// Parses the `confidence="…"` attribute value from an opening `<opinion>` tag.
fn extract_confidence(tag: &str) -> Option<f32> {
    let attr = "confidence=\"";
    let start = tag.find(attr)?;
    let start = start + attr.len();
    let end = tag[start..].find('"')?;
    tag[start..start + end].parse().ok()
}
