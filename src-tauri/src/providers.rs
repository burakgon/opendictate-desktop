//! Speech-to-text and LLM clients for OpenAI-compatible APIs (OpenRouter, OpenAI, Groq, ...).

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};

use crate::settings::{ProviderConfig, Providers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SttStyle {
    /// OpenRouter: JSON body with base64 `input_audio`.
    OpenRouterJson,
    /// OpenAI-style multipart upload to `/audio/transcriptions`.
    Multipart,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub stt: SttStyle,
    pub llm: bool,
    pub key_url: &'static str,
    pub key_hint: &'static str,
    pub needs_key: bool,
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        stt: SttStyle::OpenRouterJson,
        llm: true,
        key_url: "https://openrouter.ai/keys",
        key_hint: "sk-or-v1-...",
        needs_key: true,
    },
    ProviderInfo {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        stt: SttStyle::Multipart,
        llm: true,
        key_url: "https://platform.openai.com/api-keys",
        key_hint: "sk-...",
        needs_key: true,
    },
    ProviderInfo {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        stt: SttStyle::Multipart,
        llm: true,
        key_url: "https://console.groq.com/keys",
        key_hint: "gsk_...",
        needs_key: true,
    },
    ProviderInfo {
        id: "mistral",
        name: "Mistral",
        base_url: "https://api.mistral.ai/v1",
        stt: SttStyle::Multipart,
        llm: true,
        key_url: "https://console.mistral.ai/api-keys",
        key_hint: "",
        needs_key: true,
    },
    ProviderInfo {
        id: "gemini",
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        stt: SttStyle::None,
        llm: true,
        key_url: "https://aistudio.google.com/apikey",
        key_hint: "AIza...",
        needs_key: true,
    },
    ProviderInfo {
        id: "anthropic",
        name: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        stt: SttStyle::None,
        llm: true,
        key_url: "https://console.anthropic.com/settings/keys",
        key_hint: "sk-ant-...",
        needs_key: true,
    },
    ProviderInfo {
        id: "custom",
        name: "Custom (OpenAI-compatible)",
        base_url: "http://localhost:8000/v1",
        stt: SttStyle::Multipart,
        llm: true,
        key_url: "",
        key_hint: "optional",
        needs_key: false,
    },
];

pub fn info(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// A resolved endpoint: base URL + key + behaviour.
pub struct Endpoint {
    pub info: &'static ProviderInfo,
    pub base_url: String,
    pub api_key: String,
}

impl Endpoint {
    pub fn resolve(providers: &Providers, id: &str) -> Result<Self> {
        let info = info(id).ok_or_else(|| anyhow!("unknown provider '{id}'"))?;
        let cfg: &ProviderConfig = providers.get(id).ok_or_else(|| anyhow!("unknown provider '{id}'"))?;
        let base_url = if cfg.base_url.trim().is_empty() { info.base_url.to_string() } else { normalize_base_url(&cfg.base_url) };
        let api_key = cfg.api_key.trim().to_string();
        if info.needs_key && api_key.is_empty() {
            bail!("No API key set for {}. Add one in Settings → Providers.", info.name);
        }
        Ok(Self { info, base_url: base_url.trim_end_matches('/').to_string(), api_key })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut req = req;
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        match self.info.id {
            "openrouter" => req
                .header("HTTP-Referer", "https://github.com/burakgon/opendictate-desktop")
                .header("X-Title", "OpenDictate"),
            "anthropic" => req.header("x-api-key", &self.api_key).header("anthropic-version", "2023-06-01"),
            _ => req,
        }
    }
}

/// Accepts pasted endpoint URLs such as `http://host:8000/v1/audio/transcriptions` and
/// reduces them to the API root.
pub fn normalize_base_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    for suffix in ["/audio/transcriptions", "/audio/translations", "/chat/completions", "/completions", "/responses", "/models"] {
        if u.to_lowercase().ends_with(suffix) {
            u.truncate(u.len() - suffix.len());
            break;
        }
    }
    u.trim_end_matches('/').to_string()
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("OpenDictate/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client")
}

/// Opens (and pools) a TLS connection ahead of time so the real request starts faster.
pub async fn warm_up(http: &reqwest::Client, ep: &Endpoint) {
    let _ = http.head(&ep.base_url).timeout(Duration::from_secs(5)).send().await;
}

async fn error_from(resp: reqwest::Response, what: &str) -> anyhow::Error {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .or_else(|| v.get("detail"))
                .map(|m| match m {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    let hint = match status.as_u16() {
        401 | 403 => " - check your API key",
        402 => " - your account is out of credits",
        404 => " - check the model name and base URL",
        429 => " - rate limited, try again in a moment",
        _ => "",
    };
    anyhow!("{what} failed ({status}){hint}: {detail}")
}

pub struct SttRequest<'a> {
    pub model: &'a str,
    pub wav: Vec<u8>,
    pub language: Option<&'a str>,
    pub prompt: Option<&'a str>,
}

/// Statuses worth retrying: rate limits and transient upstream failures.
fn retryable(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// How long to wait before attempt `attempt` (1-based), honouring a provider's hint.
async fn backoff(resp: reqwest::Response, attempt: u32) -> Duration {
    let header = resp
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok());
    let body: Option<Value> = resp.json().await.ok();
    let hinted = header.or_else(|| {
        body.as_ref()
            .and_then(|v| v.pointer("/error/metadata/retry_after_seconds").or_else(|| v.pointer("/error/metadata/retry_after_seconds_raw")))
            .and_then(|v| v.as_f64())
    });
    let default = 0.6 * f64::from(attempt) * f64::from(attempt);
    Duration::from_secs_f64(hinted.unwrap_or(default).clamp(0.3, 4.0))
}

const ATTEMPTS: u32 = 3;

pub async fn transcribe(http: &reqwest::Client, ep: &Endpoint, req: SttRequest<'_>) -> Result<String> {
    if req.model.trim().is_empty() {
        bail!("No speech-to-text model selected");
    }
    let timeout = Duration::from_secs(30 + (req.wav.len() as u64 / 32_000) / 2);
    let build = || -> Result<reqwest::RequestBuilder> {
        Ok(match ep.info.stt {
            SttStyle::None => bail!("{} does not offer speech-to-text. Pick another provider for transcription.", ep.info.name),
            SttStyle::OpenRouterJson => {
                let mut body = json!({
                    "model": req.model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&req.wav),
                        "format": "wav",
                    },
                    "temperature": 0,
                });
                if let Some(lang) = req.language {
                    body["language"] = json!(lang);
                }
                ep.auth(ep.http_post(http, "audio/transcriptions")).json(&body).timeout(timeout)
            }
            SttStyle::Multipart => {
                let part = reqwest::multipart::Part::bytes(req.wav.clone()).file_name("audio.wav").mime_str("audio/wav")?;
                let mut form = reqwest::multipart::Form::new()
                    .part("file", part)
                    .text("model", req.model.to_string())
                    .text("response_format", "json")
                    .text("temperature", "0");
                if let Some(lang) = req.language {
                    form = form.text("language", lang.to_string());
                }
                if let Some(prompt) = req.prompt {
                    form = form.text("prompt", prompt.to_string());
                }
                ep.auth(ep.http_post(http, "audio/transcriptions")).multipart(form).timeout(timeout)
            }
        })
    };
    let mut attempt = 1;
    let resp = loop {
        let resp = build()?.send().await?;
        if retryable(resp.status()) && attempt < ATTEMPTS {
            let wait = backoff(resp, attempt).await;
            log::warn!("transcription attempt {attempt} was rate limited or failed upstream; retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            attempt += 1;
            continue;
        }
        break resp;
    };
    if !resp.status().is_success() {
        return Err(error_from(resp, "Transcription").await);
    }
    let v: Value = resp.json().await?;
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("unexpected transcription response: {}", v.to_string().chars().take(200).collect::<String>()))?;
    Ok(text.trim().to_string())
}

impl Endpoint {
    fn http_post(&self, http: &reqwest::Client, path: &str) -> reqwest::RequestBuilder {
        http.post(self.url(path))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

pub fn msg(role: &'static str, content: impl Into<String>) -> ChatMessage {
    ChatMessage { role, content: content.into() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reasoning {
    /// Ask the model not to think (fastest). Steps up automatically if a model insists.
    Off,
    Low,
    /// Don't send any reasoning parameter.
    Default,
}

/// Candidate request variants for a reasoning preference, tried in order.
fn reasoning_variants(provider: &str, pref: Reasoning) -> Vec<Option<(&'static str, Value)>> {
    match (provider, pref) {
        (_, Reasoning::Default) => vec![None],
        ("openrouter", Reasoning::Off) => vec![
            Some(("reasoning", json!({ "enabled": false }))),
            Some(("reasoning", json!({ "effort": "minimal" }))),
            None,
        ],
        ("openrouter", Reasoning::Low) => vec![Some(("reasoning", json!({ "effort": "low" }))), None],
        ("openai", Reasoning::Off) => vec![
            Some(("reasoning_effort", json!("none"))),
            Some(("reasoning_effort", json!("minimal"))),
            None,
        ],
        ("openai", Reasoning::Low) => vec![Some(("reasoning_effort", json!("low"))), None],
        _ => vec![None],
    }
}

/// Remembers which reasoning variant a model accepted, so later requests don't retry.
fn reasoning_cache() -> &'static parking_lot::Mutex<std::collections::HashMap<String, usize>> {
    static CACHE: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashMap<String, usize>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub async fn chat(
    http: &reqwest::Client,
    ep: &Endpoint,
    model: &str,
    messages: &[ChatMessage],
    temperature: Option<f32>,
    reasoning: Reasoning,
) -> Result<String> {
    if model.trim().is_empty() {
        bail!("No language model selected");
    }
    let variants = reasoning_variants(ep.info.id, reasoning);
    let cache_key = format!("{}:{model}:{reasoning:?}", ep.info.id);
    let start = reasoning_cache().lock().get(&cache_key).copied().unwrap_or(0).min(variants.len() - 1);
    let mut temperature = temperature;

    let mut idx = start;
    let mut attempt = 1;
    loop {
        let mut body = json!({ "model": model, "messages": messages, "stream": false });
        if let Some(t) = temperature {
            body["temperature"] = json!(t);
        }
        if let Some((key, value)) = &variants[idx] {
            body[*key] = value.clone();
        }
        let resp = ep
            .auth(ep.http_post(http, "chat/completions"))
            .json(&body)
            .timeout(Duration::from_secs(90))
            .send()
            .await?;
        if retryable(resp.status()) && attempt < ATTEMPTS {
            let wait = backoff(resp, attempt).await;
            log::warn!("language model attempt {attempt} was rate limited or failed upstream; retrying in {wait:?}");
            tokio::time::sleep(wait).await;
            attempt += 1;
            continue;
        }
        if resp.status() == reqwest::StatusCode::BAD_REQUEST || resp.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let lower = text.to_lowercase();
            if lower.contains("reasoning") && idx + 1 < variants.len() {
                // e.g. "Reasoning is mandatory for this endpoint and cannot be disabled."
                log::info!("'{model}' rejected reasoning variant {idx}; stepping up");
                idx += 1;
                continue;
            }
            if lower.contains("temperature") && temperature.is_some() {
                // Some reasoning models only accept the default temperature.
                temperature = None;
                continue;
            }
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| text.chars().take(300).collect());
            bail!("Language model request failed ({status}): {detail}");
        }
        if !resp.status().is_success() {
            return Err(error_from(resp, "Language model request").await);
        }
        if idx != start {
            reasoning_cache().lock().insert(cache_key, idx);
        }
        let v: Value = resp.json().await?;
        let content = v.pointer("/choices/0/message/content");
        let text = match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            _ => {
                if let Some(err) = v.pointer("/error/message").and_then(Value::as_str) {
                    bail!("Language model error: {err}");
                }
                bail!("The model returned an empty response")
            }
        };
        return Ok(clean_llm_output(&text));
    }
}

/// Strips wrappers models like to add even when told not to.
pub fn clean_llm_output(text: &str) -> String {
    let cleaned = strip_wrappers(text);
    // Markdown-style hard breaks ("two trailing spaces") look like stray spaces when typed.
    cleaned.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

fn strip_wrappers(text: &str) -> String {
    let mut t = text.trim();
    // Drop <think>...</think> blocks emitted inline by some reasoning models.
    let owned;
    if let Some(end) = t.find("</think>") {
        if t.starts_with("<think>") {
            owned = t[end + "</think>".len()..].trim().to_string();
            t = &owned;
        }
    }
    // Remove a single fenced block wrapping the whole answer.
    if t.starts_with("```") && t.ends_with("```") && t.len() > 6 {
        let inner = &t[3..t.len() - 3];
        let inner = match inner.find('\n') {
            Some(nl) if !inner[..nl].contains(' ') => &inner[nl + 1..],
            _ => inner,
        };
        return inner.trim().to_string();
    }
    t.to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// Human-readable price, when the provider publishes comparable pricing.
    pub price: Option<String>,
}

/// "$0.10 / $0.50" per million input/output tokens, from OpenRouter's per-token prices.
fn token_price(m: &Value) -> Option<String> {
    let per_m = |key: &str| -> Option<f64> {
        let v = m.pointer(&format!("/pricing/{key}"))?;
        let x = v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64())?;
        (x >= 0.0).then_some(x * 1_000_000.0)
    };
    let (i, o) = (per_m("prompt")?, per_m("completion")?);
    if i == 0.0 && o == 0.0 {
        return Some("free".into());
    }
    let fmt = |x: f64| if x >= 10.0 { format!("${x:.0}") } else if x >= 1.0 { format!("${x:.2}") } else { format!("${x:.3}").trim_end_matches('0').to_string() };
    Some(format!("{} / {} per M tokens", fmt(i), fmt(o)))
}

/// Lists models for a provider. `kind` is "stt" or "llm".
pub async fn list_models(http: &reqwest::Client, ep: &Endpoint, kind: &str) -> Result<Vec<ModelInfo>> {
    let path = match (ep.info.id, kind) {
        ("openrouter", "stt") => "models?output_modalities=transcription",
        _ => "models",
    };
    let resp = ep.auth(http.get(ep.url(path))).timeout(Duration::from_secs(20)).send().await?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Listing models").await);
    }
    let v: Value = resp.json().await?;
    let items = v.get("data").or_else(|| v.get("models")).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out: Vec<ModelInfo> = items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").or_else(|| m.get("name")).and_then(Value::as_str)?;
            let id = id.strip_prefix("models/").unwrap_or(id).to_string();
            let name = m
                .get("name")
                .or_else(|| m.get("display_name"))
                .and_then(Value::as_str)
                .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
                .unwrap_or_else(|| id.clone());
            if ep.info.id == "openrouter" && kind == "llm" {
                let out_mods = m.pointer("/architecture/output_modalities").and_then(Value::as_array);
                if let Some(mods) = out_mods {
                    if !mods.iter().any(|x| x == "text") {
                        return None;
                    }
                }
            }
            let price = if ep.info.id == "openrouter" && kind == "llm" { token_price(m) } else { None };
            Some(ModelInfo { id, name, price })
        })
        .filter(|m| {
            let id = m.id.to_lowercase();
            let is_stt = id.contains("whisper") || id.contains("transcribe") || id.contains("stt") || id.contains("asr");
            match (ep.info.id, kind) {
                ("openrouter", _) => true,
                (_, "stt") => is_stt || ep.info.id == "custom",
                (_, _) => {
                    !is_stt
                        && !id.contains("tts")
                        && !id.contains("embed")
                        && !id.contains("dall-e")
                        && !id.contains("image")
                        && !id.contains("moderation")
                        && !id.contains("realtime")
                        && !id.contains("audio")
                }
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

/// Checks that a key works by listing models.
pub async fn test_provider(http: &reqwest::Client, ep: &Endpoint) -> Result<usize> {
    if ep.info.id == "openrouter" {
        // /models is public on OpenRouter; /key actually validates the key.
        let resp = ep.auth(http.get(ep.url("key"))).timeout(Duration::from_secs(15)).send().await?;
        if !resp.status().is_success() {
            return Err(error_from(resp, "Key check").await);
        }
    }
    let models = list_models(http, ep, "llm").await?;
    Ok(models.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_token_prices() {
        let m = json!({"pricing": {"prompt": "0.0000001", "completion": "0.0000005"}});
        assert_eq!(token_price(&m).as_deref(), Some("$0.1 / $0.5 per M tokens"));
        let m = json!({"pricing": {"prompt": "0.000002", "completion": "0.00001"}});
        assert_eq!(token_price(&m).as_deref(), Some("$2.00 / $10 per M tokens"));
        let m = json!({"pricing": {"prompt": "0", "completion": "0"}});
        assert_eq!(token_price(&m).as_deref(), Some("free"));
        assert_eq!(token_price(&json!({"pricing": {"prompt": "-1", "completion": "-1"}})), None);
    }

    #[test]
    fn normalizes_pasted_endpoints() {
        assert_eq!(normalize_base_url("http://box:8000/v1/audio/transcriptions"), "http://box:8000/v1");
        assert_eq!(normalize_base_url("http://localhost:11434/v1/chat/completions/"), "http://localhost:11434/v1");
        assert_eq!(normalize_base_url(" https://api.groq.com/openai/v1/ "), "https://api.groq.com/openai/v1");
    }

    #[test]
    fn reasoning_steps_up() {
        assert_eq!(reasoning_variants("openrouter", Reasoning::Off).len(), 3);
        assert_eq!(reasoning_variants("groq", Reasoning::Off), vec![None]);
        assert_eq!(reasoning_variants("openai", Reasoning::Default), vec![None]);
    }

    #[test]
    fn strips_fences_and_think() {
        assert_eq!(clean_llm_output("```\nhello\n```"), "hello");
        assert_eq!(clean_llm_output("```text\nhello world\n```"), "hello world");
        assert_eq!(clean_llm_output("<think>hmm</think>\n\nAnswer"), "Answer");
        assert_eq!(clean_llm_output("  plain  "), "plain");
        assert_eq!(clean_llm_output("Red leaves  \nAutumn wind  \nEarth"), "Red leaves\nAutumn wind\nEarth");
    }
}
