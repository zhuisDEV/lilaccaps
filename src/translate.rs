use std::env;
use std::path::Path;
use std::time::Duration;

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
    if env::var_os(GEMINI_API_KEY_ENV).is_none() {
        load_dotenv(runtime_home)?;
    }
    let api_key = env::var(GEMINI_API_KEY_ENV).with_context(|| {
        format!(
            "{GEMINI_API_KEY_ENV} is not set. Add it to your environment or {}/.env.",
            runtime_home.display()
        )
    })?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("{GEMINI_API_KEY_ENV} is set but empty");
    }
    validate_model_name(model)?;

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to construct Gemini translation client")?;
    let url = format!("{GEMINI_API_BASE}/{model}:generateContent");

    let request = GenerateContentRequest {
        system_instruction: Content {
            parts: vec![Part {
                text: "Return only valid JSON with the shape {\"translations\":[...]} and no markdown. Preserve the number and order of items exactly. Each translation must be natural subtitle text in the requested target language.".to_string(),
            }],
        },
        contents: vec![Content {
            parts: vec![Part {
                text: build_translation_prompt(target_language, lines)?,
            }],
        }],
        generation_config: GenerationConfig {
            response_mime_type: "application/json",
        },
    };

    let response = client
        .post(url)
        .header("x-goog-api-key", api_key)
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

    translations
        .translations
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let line = line.trim().to_string();
            if line.is_empty() {
                bail!(
                    "Gemini returned an empty translation for subtitle line {}",
                    index + 1
                );
            }
            Ok(line)
        })
        .collect()
}

fn load_dotenv(runtime_home: &Path) -> Result<()> {
    let runtime_env = runtime_home.join(".env");
    if runtime_env.exists() {
        dotenvy::from_path(&runtime_env)
            .with_context(|| format!("failed to load {}", runtime_env.display()))?;
    }
    Ok(())
}

fn validate_model_name(model: &str) -> Result<()> {
    if model.is_empty()
        || !model.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        bail!("invalid Gemini model name: {model}");
    }
    Ok(())
}

fn build_translation_prompt(target_language: &str, lines: &[String]) -> Result<String> {
    let json_lines = serde_json::to_string(lines).context("failed to encode subtitle lines")?;
    Ok(format!(
        "Translate each subtitle line into {target_language}. Keep the meaning concise and subtitle-friendly. Return JSON only with this exact shape: {{\"translations\": [\"...\"]}}. Input lines: {json_lines}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_translation_prompt, validate_model_name};

    #[test]
    fn validates_gemini_model_name() {
        assert!(validate_model_name("gemini-3.1-flash-lite").is_ok());
        assert!(validate_model_name("gemini/latest?key=secret").is_err());
        assert!(validate_model_name("").is_err());
    }

    #[test]
    fn prompt_serializes_lines_as_json() {
        let prompt = build_translation_prompt("ja", &["hello \"world\"".to_string()])
            .expect("prompt should build");
        assert!(prompt.contains("hello \\\"world\\\""));
        assert!(prompt.contains("into ja"));
    }
}
