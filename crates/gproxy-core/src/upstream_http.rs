use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use wreq::Client as WreqClient;
use wreq::Proxy;
use wreq::header::{ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue};
use wreq_util::Emulation;

const CLIENT_CONNECT_TIMEOUT_SECS: u64 = 600;
const CLIENT_STREAM_IDLE_TIMEOUT_SECS: u64 = 3600;

pub const DEFAULT_SPOOF_EMULATION: &str = "chrome_136";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum SpoofEmulation {
    #[serde(rename = "chrome_136")]
    #[default]
    Chrome136,
    #[serde(rename = "chrome_137")]
    Chrome137,
    #[serde(rename = "chrome_138")]
    Chrome138,
    #[serde(rename = "edge_136")]
    Edge136,
    #[serde(rename = "edge_137")]
    Edge137,
    #[serde(rename = "firefox_136")]
    Firefox136,
    #[serde(rename = "firefox_139")]
    Firefox139,
    #[serde(rename = "safari_18_5")]
    Safari18_5,
}

impl SpoofEmulation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chrome136 => "chrome_136",
            Self::Chrome137 => "chrome_137",
            Self::Chrome138 => "chrome_138",
            Self::Edge136 => "edge_136",
            Self::Edge137 => "edge_137",
            Self::Firefox136 => "firefox_136",
            Self::Firefox139 => "firefox_139",
            Self::Safari18_5 => "safari_18_5",
        }
    }

    pub const fn variants() -> &'static [&'static str] {
        &[
            "chrome_136",
            "chrome_137",
            "chrome_138",
            "edge_136",
            "edge_137",
            "firefox_136",
            "firefox_139",
            "safari_18_5",
        ]
    }

    pub const fn into_wreq_emulation(self) -> Emulation {
        match self {
            Self::Chrome136 => Emulation::Chrome136,
            Self::Chrome137 => Emulation::Chrome137,
            Self::Chrome138 => Emulation::Chrome138,
            Self::Edge136 => Emulation::Edge136,
            Self::Edge137 => Emulation::Edge137,
            Self::Firefox136 => Emulation::Firefox136,
            Self::Firefox139 => Emulation::Firefox139,
            Self::Safari18_5 => Emulation::Safari18_5,
        }
    }
}

impl FromStr for SpoofEmulation {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase().replace('.', "_");
        match normalized.as_str() {
            "chrome_136" | "chrome136" => Ok(Self::Chrome136),
            "chrome_137" | "chrome137" => Ok(Self::Chrome137),
            "chrome_138" | "chrome138" => Ok(Self::Chrome138),
            "edge_136" | "edge136" => Ok(Self::Edge136),
            "edge_137" | "edge137" => Ok(Self::Edge137),
            "firefox_136" | "firefox136" => Ok(Self::Firefox136),
            "firefox_139" | "firefox139" => Ok(Self::Firefox139),
            "safari_18_5" | "safari18_5" | "safari185" => Ok(Self::Safari18_5),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub enum UpstreamHttpClientBuildError {
    InvalidSpoofEmulation(String),
    Wreq(wreq::Error),
}

impl fmt::Display for UpstreamHttpClientBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpoofEmulation(value) => write!(
                f,
                "invalid spoof emulation '{value}', allowed: {}",
                SpoofEmulation::variants().join(", ")
            ),
            Self::Wreq(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for UpstreamHttpClientBuildError {}

impl From<wreq::Error> for UpstreamHttpClientBuildError {
    fn from(value: wreq::Error) -> Self {
        Self::Wreq(value)
    }
}

fn normalize_proxy(proxy: Option<&str>) -> Option<&str> {
    proxy.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then_some(value)
    })
}

pub fn normalize_spoof_emulation(value: Option<&str>) -> String {
    let Some(raw) = value else {
        return DEFAULT_SPOOF_EMULATION.to_string();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_SPOOF_EMULATION.to_string();
    }
    SpoofEmulation::from_str(trimmed)
        .map(SpoofEmulation::as_str)
        .unwrap_or(trimmed)
        .to_string()
}

pub fn build_http_client(proxy: Option<&str>) -> Result<WreqClient, UpstreamHttpClientBuildError> {
    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS));

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

pub fn build_claudecode_spoof_client(
    proxy: Option<&str>,
    spoof_emulation: &str,
) -> Result<WreqClient, UpstreamHttpClientBuildError> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    default_headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    default_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    let spoof_emulation = SpoofEmulation::from_str(spoof_emulation.trim()).map_err(|_| {
        UpstreamHttpClientBuildError::InvalidSpoofEmulation(spoof_emulation.to_string())
    })?;

    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS))
        .cookie_store(true)
        .emulation(spoof_emulation.into_wreq_emulation())
        .default_headers(default_headers);

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}
