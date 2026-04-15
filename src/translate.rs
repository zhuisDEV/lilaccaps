use std::env;
use std::path::Path;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const GEMINI_API_KEY_ENV: &str = "GEMINI_API_KEY";

#[derive(Debug, Serialize)]
struct GenerateContentRequest {
    #[serde(rename = "systemInstruction")]
    system_instruction: Content,
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Part {
    text: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: &'static str,
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Deserialize)]
struct TranslationPayload {
    translations: Vec<String>,
}

pub fn translate_lines(
    runtime_home: &Path,
    model: &str,
    target_language: &str,
    lines: &[String],
) -> Result<Vec<String>> {
    load_dotenv(runtime_home);
    let api_key = env::var(GEMINI_API_KEY_ENV).with_context(|| {
        format!(
            "{GEMINI_API_KEY_ENV} is not set. Add it to your environment or {}/.env.",
            runtime_home.display()
        )
    })?;

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to construct Gemini translation client")?;
    let url = format!("{GEMINI_API_BASE}/{model}:generateContent?key={api_key}");

    let request = GenerateContentRequest {
        system_instruction: Content {
            parts: vec![Part {
                text: "Return only valid JSON with the shape {\"translations\":[...]} and no markdown. Preserve the number and order of items exactly. Each translation must be natural subtitle text in the requested target language.".to_string(),
            }],
        },
        contents: vec![Content {
            parts: vec![Part {
                text: build_translation_prompt(target_language, lines),
            }],
        }],
        generation_config: GenerationConfig {
            response_mime_type: "application/json",
        },
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .context("failed to call Gemini translation API")?
        .error_for_status()
        .context("Gemini translation API returned an error")?;

    let payload: GenerateContentResponse = response
        .json()
        .context("failed to decode Gemini translation response")?;
    let text = payload
        .candidates
        .first()
        .and_then(|candidate| candidate.content.parts.first())
        .map(|part| part.text.as_str())
        .ok_or_else(|| anyhow::anyhow!("Gemini translation response contained no text content"))?;
    let translations: TranslationPayload =
        serde_json::from_str(text).context("failed to parse Gemini translation JSON payload")?;

    if translations.translations.len() != lines.len() {
        bail!(
            "Gemini returned {} translations for {} subtitle lines",
            translations.translations.len(),
            lines.len()
        );
    }

    Ok(translations
        .translations
        .into_iter()
        .map(|line| line.trim().to_string())
        .collect())
}

fn load_dotenv(runtime_home: &Path) {
    let runtime_env = runtime_home.join(".env");
    if runtime_env.exists() {
        let _ = dotenvy::from_path_override(runtime_env);
        return;
    }

    let _ = dotenvy::dotenv_override();
}

fn build_translation_prompt(target_language: &str, lines: &[String]) -> String {
    let json_lines = serde_json::to_string(lines).expect("subtitle lines should serialize");
    format!(
        "Translate each subtitle line into {target_language}. Keep the meaning concise and subtitle-friendly. Return JSON only with this exact shape: {{\"translations\": [\"...\"]}}. Input lines: {json_lines}"
    )
}
