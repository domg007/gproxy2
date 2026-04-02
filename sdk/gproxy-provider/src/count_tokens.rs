use crate::engine::Usage;

use std::sync::{Arc, OnceLock};
use tiktoken_rs::{CoreBPE, get_bpe_from_model, o200k_base};
use tokenizers::Tokenizer;

/// Token counting strategy, tried in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountStrategy {
    /// Call upstream count_tokens API.
    UpstreamApi,
    /// Use local tokenizer (tiktoken for GPT models, DeepSeek/HF for others).
    Local,
}

/// Result of a token count operation.
#[derive(Debug, Clone)]
pub struct TokenCount {
    pub count: i64,
    pub method: CountMethod,
}

/// How the token count was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountMethod {
    UpstreamApi,
    LocalTiktoken,
    LocalDeepSeek,
}

const DEEPSEEK_TOKENIZER_BYTES: &[u8] = include_bytes!("tokenizers/deepseek_tokenizer.json");

/// Count tokens locally. Tries tiktoken first (GPT models), falls back to
/// bundled DeepSeek tokenizer (covers all other models).
/// Always succeeds — DeepSeek tokenizer is the universal fallback.
pub fn count_tokens_local(model: &str, text: &str) -> TokenCount {
    if is_gpt_model(model)
        && let Ok(count) = count_tiktoken(model, text)
    {
        return TokenCount {
            count,
            method: CountMethod::LocalTiktoken,
        };
    }

    let count = count_deepseek(text);
    TokenCount {
        count,
        method: CountMethod::LocalDeepSeek,
    }
}

/// Estimate output tokens from partially received streaming body.
/// Used when stream is interrupted and no usage was reported.
pub fn estimate_partial_usage(
    input_tokens: Option<i64>,
    partial_output: &str,
    model: &str,
) -> Usage {
    let tc = count_tokens_local(model, partial_output);
    Usage {
        input_tokens,
        output_tokens: Some(tc.count),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

// === Tiktoken (GPT models) ===

fn is_gpt_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("gpt")
        || m.starts_with("chatgpt")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("ft:gpt")
        || m.contains("gpt-")
}

fn count_tiktoken(model: &str, text: &str) -> Result<i64, String> {
    let bpe = build_bpe(model)?;
    Ok(bpe.encode_ordinary(text).len() as i64)
}

fn build_bpe(model: &str) -> Result<CoreBPE, String> {
    if let Ok(bpe) = get_bpe_from_model(model) {
        return Ok(bpe);
    }
    o200k_base().map_err(|e| e.to_string())
}

// === DeepSeek tokenizer (universal fallback) ===

fn deepseek_tokenizer() -> &'static Arc<Tokenizer> {
    static TOKENIZER: OnceLock<Arc<Tokenizer>> = OnceLock::new();
    TOKENIZER.get_or_init(|| {
        let tokenizer = Tokenizer::from_bytes(DEEPSEEK_TOKENIZER_BYTES)
            .expect("bundled DeepSeek tokenizer must be valid");
        Arc::new(tokenizer)
    })
}

fn count_deepseek(text: &str) -> i64 {
    let tokenizer = deepseek_tokenizer();
    match tokenizer.encode(text, false) {
        Ok(encoding) => encoding.len() as i64,
        Err(_) => {
            // Should never fail with valid UTF-8, but just in case
            (text.len() as i64 + 2) / 3
        }
    }
}
