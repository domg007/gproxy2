pub mod huggingface;
pub mod tiktoken;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;
use wreq::Client as WreqClient;

use self::huggingface::{
    HuggingFaceTokenizerSource, load_or_download_hf_tokenizer, load_tokenizer_from_file,
};
use self::tiktoken::{count_tiktoken_tokens, is_gpt_like_model};

const DEEPSEEK_FALLBACK_KEY: &str = "deepseek_fallback";
const DEEPSEEK_FALLBACK_TOKENIZER_BYTES: &[u8] = include_bytes!("deepseek_tokenizer.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalTokenizerBackend {
    TikToken,
    Tokenizers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTokenCount {
    pub count: usize,
    pub backend: LocalTokenizerBackend,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalTokenizerError {
    #[error("empty model")]
    EmptyModel,
    #[error("invalid tokenizer bytes for model {model}: {message}")]
    InvalidMemoryTokenizer { model: String, message: String },
    #[error("tokenizer encode failed for model {model}: {message}")]
    Encode { model: String, message: String },
    #[error("tokenizer download failed for model {model}: {message}")]
    Download { model: String, message: String },
    #[error("tokenizer file error for model {model}: {message}")]
    File { model: String, message: String },
    #[error("tiktoken failed for model {model}: {message}")]
    TikToken { model: String, message: String },
}

#[derive(Debug, Clone)]
pub struct LocalTokenizerStore {
    cache_dir: Arc<PathBuf>,
    tokenizers: Arc<DashMap<String, Arc<Tokenizer>>>,
}

impl LocalTokenizerStore {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: Arc::new(cache_dir.into()),
            tokenizers: Arc::new(DashMap::new()),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        self.cache_dir.as_ref()
    }

    pub fn upsert_memory_tokenizer_bytes(
        &self,
        model: impl Into<String>,
        tokenizer_json: Vec<u8>,
    ) -> Result<(), LocalTokenizerError> {
        let model = model.into().trim().to_string();
        if model.is_empty() {
            return Err(LocalTokenizerError::EmptyModel);
        }

        let tokenizer = Tokenizer::from_bytes(tokenizer_json.as_slice()).map_err(|err| {
            LocalTokenizerError::InvalidMemoryTokenizer {
                model: model.clone(),
                message: err.to_string(),
            }
        })?;
        let tokenizer = Arc::new(tokenizer);
        self.tokenizers.insert(model, tokenizer);
        Ok(())
    }

    pub fn remove_memory_tokenizer(&self, model: &str) {
        self.tokenizers.remove(model);
    }

    pub async fn count_text_tokens(
        &self,
        http_client: &WreqClient,
        hf_token: Option<&str>,
        hf_base_url: Option<&str>,
        model: &str,
        text: &str,
    ) -> Result<LocalTokenCount, LocalTokenizerError> {
        let model = model.trim();
        if model.is_empty() {
            return Err(LocalTokenizerError::EmptyModel);
        }

        if is_gpt_like_model(model) {
            let count = count_tiktoken_tokens(model, text).map_err(|err| {
                LocalTokenizerError::TikToken {
                    model: model.to_string(),
                    message: err.to_string(),
                }
            })?;
            return Ok(LocalTokenCount {
                count,
                backend: LocalTokenizerBackend::TikToken,
            });
        }

        let tokenizer = self
            .resolve_tokenizer(http_client, hf_token, hf_base_url, model)
            .await?;
        let encoding =
            tokenizer
                .encode(text, false)
                .map_err(|err| LocalTokenizerError::Encode {
                    model: model.to_string(),
                    message: err.to_string(),
                })?;
        Ok(LocalTokenCount {
            count: encoding.len(),
            backend: LocalTokenizerBackend::Tokenizers,
        })
    }

    pub fn ensure_deepseek_fallback(&self) -> Result<(), LocalTokenizerError> {
        let tokenizer = embedded_deepseek_tokenizer()?;
        self.tokenizers
            .insert(DEEPSEEK_FALLBACK_KEY.to_string(), tokenizer);
        Ok(())
    }

    async fn resolve_tokenizer(
        &self,
        http_client: &WreqClient,
        hf_token: Option<&str>,
        hf_base_url: Option<&str>,
        model: &str,
    ) -> Result<Arc<Tokenizer>, LocalTokenizerError> {
        if let Some(tokenizer) = self.tokenizers.get(model) {
            let tokenizer = tokenizer.value().clone();
            return Ok(tokenizer);
        }

        let source = HuggingFaceTokenizerSource::from_model(model);
        if let Some(tokenizer) = load_tokenizer_from_file(self.cache_dir.as_ref(), &source)? {
            self.tokenizers.insert(model.to_string(), tokenizer.clone());
            return Ok(tokenizer);
        }

        match load_or_download_hf_tokenizer(
            http_client,
            hf_token,
            hf_base_url,
            self.cache_dir.as_ref(),
            &source,
        )
        .await
        {
            Ok(tokenizer) => {
                self.tokenizers.insert(model.to_string(), tokenizer.clone());
                Ok(tokenizer)
            }
            Err(primary_err) => {
                let tokenizer = embedded_deepseek_tokenizer().map_err(|fallback_err| {
                    LocalTokenizerError::Download {
                        model: model.to_string(),
                        message: format!("primary={primary_err}; deepseek_fallback={fallback_err}"),
                    }
                })?;
                self.tokenizers.insert(model.to_string(), tokenizer.clone());
                self.tokenizers
                    .insert(DEEPSEEK_FALLBACK_KEY.to_string(), tokenizer.clone());
                Ok(tokenizer)
            }
        }
    }
}

fn embedded_deepseek_tokenizer() -> Result<Arc<Tokenizer>, LocalTokenizerError> {
    static TOKENIZER: OnceLock<Result<Arc<Tokenizer>, String>> = OnceLock::new();
    let init = TOKENIZER.get_or_init(|| {
        Tokenizer::from_bytes(DEEPSEEK_FALLBACK_TOKENIZER_BYTES)
            .map(Arc::new)
            .map_err(|err| err.to_string())
    });
    match init {
        Ok(tokenizer) => Ok(tokenizer.clone()),
        Err(message) => Err(LocalTokenizerError::InvalidMemoryTokenizer {
            model: DEEPSEEK_FALLBACK_KEY.to_string(),
            message: message.clone(),
        }),
    }
}
