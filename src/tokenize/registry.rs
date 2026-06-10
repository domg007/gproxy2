//! Global HF-tokenizer registry (§6.3): bundled deepseek vocab, lazy-loaded
//! downloads under `data_dir/tokenizers/`, and a fire-and-forget background
//! HF download path through the shared [`UpstreamClient`]. Native-only
//! (`count-local` feature); tiktoken builtins are handled directly by
//! [`super::count`] and never live here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use dashmap::DashMap;
use tokenizers::Tokenizer;

use crate::http::client::UpstreamClient;

/// Bundled DeepSeek vocab, vendored from `deepseek-ai/DeepSeek-V4-Pro`
/// (`tokenizer.json`).
static DEEPSEEK: &[u8] = include_bytes!("../../assets/tokenizers/deepseek-v4-pro.tokenizer.json");
/// Names the bundled vocab answers to.
const BUNDLED_NAMES: &[&str] = &["deepseek", "deepseek-v4-pro"];

/// Where a vocab comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabSource {
    BuiltinTiktoken,
    Bundled,
    Downloaded,
}

/// Listing entry for the admin surface.
#[derive(Debug, Clone)]
pub struct VocabInfo {
    pub name: String,
    pub source: VocabSource,
    pub loaded: bool,
}

type LoadedMap = Arc<DashMap<String, Arc<Tokenizer>>>;

/// Global tokenizer registry living on `AppState`.
pub struct TokenizerRegistry {
    /// `data_dir/tokenizers`; `None` = no disk tier, downloads disabled.
    dir: Option<PathBuf>,
    /// Mirrors `instance_settings.enable_tokenizer_download`.
    download_enabled: AtomicBool,
    upstream: Arc<dyn UpstreamClient>,
    loaded: LoadedMap,
    inflight: Arc<DashMap<String, ()>>,
}

impl TokenizerRegistry {
    pub fn new(dir: Option<PathBuf>, upstream: Arc<dyn UpstreamClient>) -> Self {
        Self {
            dir,
            download_enabled: AtomicBool::new(false),
            upstream,
            loaded: Arc::new(DashMap::new()),
            inflight: Arc::new(DashMap::new()),
        }
    }

    pub fn set_download_enabled(&self, on: bool) {
        self.download_enabled.store(on, Ordering::Relaxed);
    }

    /// Builtins + bundled + on-disk downloads.
    pub fn list(&self) -> Vec<VocabInfo> {
        let mut out = vec![
            info("o200k_base", VocabSource::BuiltinTiktoken, true),
            info("cl100k_base", VocabSource::BuiltinTiktoken, true),
            info(
                BUNDLED_NAMES[0],
                VocabSource::Bundled,
                self.loaded.contains_key(BUNDLED_NAMES[0]),
            ),
        ];
        if let Some(dir) = &self.dir
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if let Some(stem) = file.strip_suffix(".json") {
                    let name = stem.replace("--", "/");
                    let loaded = self.loaded.contains_key(&name);
                    out.push(info(&name, VocabSource::Downloaded, loaded));
                }
            }
        }
        out
    }

    /// memory → bundled name → disk file (sanitized name) → `None`.
    pub fn resolve(&self, name: &str) -> Option<Arc<Tokenizer>> {
        if let Some(t) = self.loaded.get(name) {
            return Some(Arc::clone(&t));
        }
        if BUNDLED_NAMES.contains(&name) {
            let tok = Arc::new(Tokenizer::from_bytes(DEEPSEEK).ok()?);
            for n in BUNDLED_NAMES {
                self.loaded.insert((*n).to_owned(), Arc::clone(&tok));
            }
            return Some(tok);
        }
        let path = self.dir.as_ref()?.join(format!("{}.json", sanitize(name)));
        let bytes = std::fs::read(path).ok()?;
        let tok = Arc::new(Tokenizer::from_bytes(&bytes).ok()?);
        self.loaded.insert(name.to_owned(), Arc::clone(&tok));
        Some(tok)
    }

    /// Fire-and-forget: download `hf.co/{name}/resolve/main/tokenizer.json`
    /// through the shared upstream client and persist it under the registry
    /// dir. No-op when disabled, dirless, the name is not an HF repo path
    /// (`org/repo`), or a download is already inflight. Never blocks.
    pub fn request_download(&self, name: &str) {
        let Some(dir) = self.dir.clone() else { return };
        if !self.download_enabled.load(Ordering::Relaxed)
            || !name.contains('/')
            || self.inflight.insert(name.to_owned(), ()).is_some()
        {
            return;
        }
        let upstream = Arc::clone(&self.upstream);
        let loaded = Arc::clone(&self.loaded);
        let inflight = Arc::clone(&self.inflight);
        let name = name.to_owned();
        tokio::spawn(async move {
            if let Err(e) = download(upstream, &dir, &name, &loaded).await {
                tracing::warn!(name, error = %e, "tokenizer download failed");
            }
            inflight.remove(&name);
        });
    }
}

async fn download(
    upstream: Arc<dyn UpstreamClient>,
    dir: &Path,
    name: &str,
    loaded: &LoadedMap,
) -> anyhow::Result<()> {
    let url = format!("https://huggingface.co/{name}/resolve/main/tokenizer.json");
    let req = http::Request::builder()
        .method(http::Method::GET)
        .uri(&url)
        .body(Bytes::new())?;
    let resp = upstream
        .send(req)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(resp.status().is_success(), "HTTP {}", resp.status());
    let body = resp.into_body();
    let tok = Tokenizer::from_bytes(&body).map_err(|e| anyhow::anyhow!("bad vocab: {e}"))?;

    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", sanitize(name)));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, &path)?;

    loaded.insert(name.to_owned(), Arc::new(tok));
    tracing::info!(name, "tokenizer downloaded");
    Ok(())
}

fn info(name: &str, source: VocabSource, loaded: bool) -> VocabInfo {
    VocabInfo {
        name: name.to_owned(),
        source,
        loaded,
    }
}

/// HF repo paths contain `/`; keep disk names flat.
fn sanitize(name: &str) -> String {
    name.replace('/', "--")
}
