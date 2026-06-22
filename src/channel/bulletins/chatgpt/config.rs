//! Config builder for the prepare `p` field and PoW payloads.
//!
//! Ported from v1 `channels/chatgpt/prepare_p.rs` (the config core; date helpers
//! live in [`super::browser_date`]). The server accepts any structurally valid
//! 25-slot config array; the individual values (UA, timestamps, uuid, screen
//! size, etc.) are not cross-checked, but the 25-slot layout / encoding / slot
//! semantics (slot 3, slot 9) are byte-exact — any drift is rejected.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::browser_date::format_browser_date;

pub const DEFAULT_BUILD_ID: &str = "prod-d7545204e22cb990d0245281e6550977d93b6a81";
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0";
pub const DEFAULT_LANGUAGES: &[&str] = &["en", "zh-CN", "zh"];
pub const DEFAULT_TQT_CANDIDATES: &[&str] = &[
    "vendor−Google Inc.",
    "language−en",
    "wakeLock−[object WakeLock]",
    "maxTouchPoints−0",
    "deviceMemory−8",
];
pub const DEFAULT_DOCUMENT_KEYS: &[&str] = &["location", "_reactListening7emk2nodhb"];
pub const DEFAULT_WINDOW_KEYS: &[&str] = &[
    "outerWidth",
    "__oai_so_kp",
    "localStorage",
    "visualViewport",
];

/// A uniform `f64` in `[0, 1)`, matching JS `Math.random()`'s 53-bit mantissa.
fn rand_f64() -> f64 {
    let bytes = crate::util::rand::bytes::<8>();
    (u64::from_le_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64
}

/// Options for building the base config array. Fields roughly correspond to
/// JS `buildBaseConfig(options)`; defaults mirror the browser.
#[derive(Debug, Clone)]
pub struct ConfigOptions {
    pub user_agent: String,
    pub build_id: String,
    pub language: String,
    pub languages: Vec<String>,
    pub screen_width: u32,
    pub screen_height: u32,
    pub hardware_concurrency: u32,
    pub js_heap_size_limit: u64,
    pub tqt_value: String,
    pub document_key: String,
    pub window_key: String,
    pub sid: String,
    pub search_keys: String,
    pub date_string: String,
    pub performance_now: f64,
    pub time_origin: f64,
    pub rand3: f64,
    pub rand9: f64,
    pub elapsed_ms: f64,
}

impl ConfigOptions {
    /// Build with realistic browser-like defaults + a fresh uuid.
    pub fn browser_default() -> Self {
        let now_secs = crate::util::time::unix_now();
        let time_origin = crate::util::time::unix_now_ms() as f64;
        Self {
            user_agent: DEFAULT_USER_AGENT.to_string(),
            build_id: DEFAULT_BUILD_ID.to_string(),
            language: "en".to_string(),
            languages: DEFAULT_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            screen_width: 1366,
            screen_height: 1408,
            hardware_concurrency: 32,
            js_heap_size_limit: 4_294_967_296,
            tqt_value: DEFAULT_TQT_CANDIDATES[0].to_string(),
            document_key: DEFAULT_DOCUMENT_KEYS[0].to_string(),
            window_key: DEFAULT_WINDOW_KEYS[0].to_string(),
            sid: crate::util::rand::uuid_v4(),
            search_keys: String::new(),
            date_string: format_browser_date(now_secs, 480, "中国标准时间"),
            performance_now: 30412.5,
            time_origin,
            rand3: rand_f64(),
            rand9: rand_f64(),
            elapsed_ms: 0.0,
        }
    }

    /// Deterministic values for unit tests.
    #[cfg(test)]
    pub fn fixed_for_tests() -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_string(),
            build_id: DEFAULT_BUILD_ID.to_string(),
            language: "en".to_string(),
            languages: DEFAULT_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            screen_width: 1366,
            screen_height: 1408,
            hardware_concurrency: 32,
            js_heap_size_limit: 4_294_967_296,
            tqt_value: DEFAULT_TQT_CANDIDATES[0].to_string(),
            document_key: DEFAULT_DOCUMENT_KEYS[0].to_string(),
            window_key: DEFAULT_WINDOW_KEYS[0].to_string(),
            sid: "ee7b3426-19ed-4541-868a-ae24e57837ba".to_string(),
            search_keys: String::new(),
            date_string: "Tue Apr 21 2026 17:25:57 GMT+0800 (中国标准时间)".to_string(),
            performance_now: 30412.5,
            time_origin: 1776763524501.3,
            rand3: 0.12345,
            rand9: 0.67890,
            elapsed_ms: 0.0,
        }
    }
}

/// 25-slot config array matching the JS layout.
pub fn build_base_config(opts: &ConfigOptions) -> Config {
    Config {
        screen_sum: opts.screen_width + opts.screen_height,
        date_string: opts.date_string.clone(),
        js_heap_size_limit: opts.js_heap_size_limit,
        attempt: opts.rand3,
        user_agent: opts.user_agent.clone(),
        script_source: Value::Null,
        build_id: opts.build_id.clone(),
        language: opts.language.clone(),
        languages_joined: opts.languages.join(","),
        elapsed_ms: opts.rand9,
        tqt_value: opts.tqt_value.clone(),
        document_key: opts.document_key.clone(),
        window_key: opts.window_key.clone(),
        performance_now: opts.performance_now,
        sid: opts.sid.clone(),
        search_keys: opts.search_keys.clone(),
        hardware_concurrency: opts.hardware_concurrency,
        time_origin: opts.time_origin,
    }
}

/// 25-slot config. Serialized as an array in the order shown by
/// `to_json_array()`.
#[derive(Debug, Clone)]
pub struct Config {
    pub screen_sum: u32,
    pub date_string: String,
    pub js_heap_size_limit: u64,
    /// Slot 3: random float for prepare, integer attempt for PoW.
    pub attempt: f64,
    pub user_agent: String,
    pub script_source: Value,
    pub build_id: String,
    pub language: String,
    pub languages_joined: String,
    /// Slot 9: random float for prepare, elapsed ms for PoW.
    pub elapsed_ms: f64,
    pub tqt_value: String,
    pub document_key: String,
    pub window_key: String,
    pub performance_now: f64,
    pub sid: String,
    pub search_keys: String,
    pub hardware_concurrency: u32,
    pub time_origin: f64,
}

impl Config {
    fn to_json_array(&self) -> Vec<Value> {
        vec![
            json!(self.screen_sum),
            json!(self.date_string),
            json!(self.js_heap_size_limit),
            json!(self.attempt),
            json!(self.user_agent),
            self.script_source.clone(),
            json!(self.build_id),
            json!(self.language),
            json!(self.languages_joined),
            json!(self.elapsed_ms),
            json!(self.tqt_value),
            json!(self.document_key),
            json!(self.window_key),
            json!(self.performance_now),
            json!(self.sid),
            json!(self.search_keys),
            json!(self.hardware_concurrency),
            json!(self.time_origin),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
            json!(0),
        ]
    }
}

/// `base64(JSON.stringify(config))`.
pub fn encode_config(cfg: &Config) -> String {
    let arr = cfg.to_json_array();
    let s = serde_json::to_string(&arr).unwrap();
    STANDARD.encode(s.as_bytes())
}

/// Build the prepare request `p` field: `gAAAAAC<base64(json(config))>`.
/// Slot 3 is forced to `1` and slot 9 to `prepare_duration_ms`.
pub fn build_prepare_p(opts: &ConfigOptions) -> String {
    let mut cfg = build_base_config(opts);
    cfg.attempt = 1.0;
    cfg.elapsed_ms = opts.performance_now.max(0.0);
    format!("gAAAAAC{}", encode_config(&cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_p_has_expected_prefix_and_length() {
        let opts = ConfigOptions::fixed_for_tests();
        let p = build_prepare_p(&opts);
        assert!(p.starts_with("gAAAAAC"), "prefix: {}", &p[..15]);
        // Sample HAR length was 575/603; ours should be in the same ballpark.
        assert!(p.len() > 400 && p.len() < 900, "len={}", p.len());
    }

    #[test]
    fn config_decodes_to_25_slots() {
        let opts = ConfigOptions::fixed_for_tests();
        let p = build_prepare_p(&opts);
        let body = p.strip_prefix("gAAAAAC").unwrap();
        let raw = STANDARD.decode(body).unwrap();
        let arr: Vec<Value> = serde_json::from_slice(&raw).unwrap();
        assert_eq!(arr.len(), 25);
        assert_eq!(arr[3], json!(1.0));
    }
}
