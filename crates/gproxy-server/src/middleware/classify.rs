use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http_body_util::BodyExt;
use serde::Deserialize;

use crate::middleware::kinds::{OperationFamily, ProtocolKind};

/// Classification result stored in request extensions.
#[derive(Debug, Clone)]
pub struct Classification {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
}

/// Buffered request body bytes stored in extensions by classify_middleware.
/// Allows downstream middleware to read the body without consuming it.
#[derive(Debug, Clone)]
pub struct BufferedBodyBytes(pub Bytes);

/// Axum middleware: classify the request by operation and protocol,
/// buffer the body, and store `Classification` in extensions.
pub async fn classify_middleware(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();

    // Buffer body
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "failed to read request body").into_response();
        }
    };

    let path = normalize_path(parts.uri.path());
    let query = parts.uri.query();
    let method = &parts.method;
    let headers = &parts.headers;

    let result = classify_route(method, &path, query, headers, &body_bytes);

    match result {
        Ok(classification) => {
            let mut request = Request::from_parts(parts, Body::from(body_bytes.clone()));
            request.extensions_mut().insert(BufferedBodyBytes(body_bytes));
            request.extensions_mut().insert(classification);
            next.run(request).await
        }
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Classification logic
// ---------------------------------------------------------------------------

fn classify_route(
    method: &Method,
    path: &str,
    query: Option<&str>,
    headers: &http::HeaderMap,
    body: &Bytes,
) -> Result<Classification, &'static str> {
    if *method == Method::GET {
        if path == "/models" {
            return Ok(Classification {
                operation: OperationFamily::ModelList,
                protocol: classify_models_protocol(headers, query),
            });
        }
        if is_model_get_path(path) {
            return Ok(Classification {
                operation: OperationFamily::ModelGet,
                protocol: classify_models_protocol(headers, query),
            });
        }
        if path == "/files" {
            return Ok(Classification {
                operation: OperationFamily::FileList,
                protocol: ProtocolKind::OpenAi,
            });
        }
        if is_file_content_path(path) {
            return Ok(Classification {
                operation: OperationFamily::FileContent,
                protocol: ProtocolKind::OpenAi,
            });
        }
        if is_file_get_path(path) {
            return Ok(Classification {
                operation: OperationFamily::FileGet,
                protocol: ProtocolKind::OpenAi,
            });
        }
        return Err("unsupported GET path");
    }

    if *method == Method::DELETE {
        if let Some(_file_id) = extract_file_id_from_normalized(path) {
            return Ok(Classification {
                operation: OperationFamily::FileDelete,
                protocol: ProtocolKind::OpenAi,
            });
        }
        return Err("unsupported DELETE path");
    }

    if *method != Method::POST {
        return Err("unsupported HTTP method");
    }

    if path == "/files" {
        return Ok(Classification {
            operation: OperationFamily::FileUpload,
            protocol: ProtocolKind::OpenAi,
        });
    }

    if path == "/responses" {
        return Ok(Classification {
            operation: stream_or_non_stream(body),
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/chat/completions" {
        return Ok(Classification {
            operation: stream_or_non_stream(body),
            protocol: ProtocolKind::OpenAiChatCompletion,
        });
    }
    if path == "/messages" {
        return Ok(Classification {
            operation: stream_or_non_stream(body),
            protocol: ProtocolKind::Claude,
        });
    }
    if path == "/responses/input_tokens" || path == "/responses/input_tokens/count" {
        return Ok(Classification {
            operation: OperationFamily::CountToken,
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/messages/count_tokens" || path == "/messages/count-tokens" {
        return Ok(Classification {
            operation: OperationFamily::CountToken,
            protocol: ProtocolKind::Claude,
        });
    }
    if path == "/responses/compact" {
        return Ok(Classification {
            operation: OperationFamily::Compact,
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/embeddings" {
        return Ok(Classification {
            operation: OperationFamily::Embedding,
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/images/generations" {
        return Ok(Classification {
            operation: if read_stream_flag(body) {
                OperationFamily::StreamCreateImage
            } else {
                OperationFamily::CreateImage
            },
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/images/edits" {
        return Ok(Classification {
            operation: if read_stream_flag(body) {
                OperationFamily::StreamCreateImageEdit
            } else {
                OperationFamily::CreateImageEdit
            },
            protocol: ProtocolKind::OpenAi,
        });
    }
    if let Some((operation, protocol)) = classify_gemini(path, query) {
        return Ok(Classification {
            operation,
            protocol,
        });
    }

    Err("unable to classify request")
}

fn classify_models_protocol(headers: &http::HeaderMap, query: Option<&str>) -> ProtocolKind {
    if headers.contains_key("anthropic-version")
        || headers.contains_key("anthropic-beta")
        || headers.contains_key("x-api-key")
        || query_has_key(query, "after_id")
        || query_has_key(query, "before_id")
        || query_has_key(query, "limit")
    {
        return ProtocolKind::Claude;
    }
    if headers.contains_key("x-goog-api-key")
        || query_has_key(query, "pageSize")
        || query_has_key(query, "pageToken")
        || query_has_key(query, "key")
    {
        return ProtocolKind::Gemini;
    }
    ProtocolKind::OpenAi
}

fn classify_gemini(path: &str, query: Option<&str>) -> Option<(OperationFamily, ProtocolKind)> {
    let tail = path.strip_prefix("/models/")?;
    let (_, action) = tail.rsplit_once(':')?;
    match action {
        "countTokens" => Some((OperationFamily::CountToken, ProtocolKind::Gemini)),
        "generateContent" => Some((OperationFamily::GenerateContent, ProtocolKind::Gemini)),
        "streamGenerateContent" => Some((
            OperationFamily::StreamGenerateContent,
            if query_has_value(query, "alt", "sse") {
                ProtocolKind::Gemini
            } else {
                ProtocolKind::GeminiNDJson
            },
        )),
        "embedContent" => Some((OperationFamily::Embedding, ProtocolKind::Gemini)),
        _ => None,
    }
}

fn is_model_get_path(path: &str) -> bool {
    let Some(tail) = path.strip_prefix("/models/") else {
        return false;
    };
    !tail.is_empty() && !tail.contains('/') && !tail.contains(':')
}

fn is_file_get_path(path: &str) -> bool {
    extract_file_id_from_normalized(path).is_some()
}

fn is_file_content_path(path: &str) -> bool {
    let Some(tail) = path.strip_prefix("/files/") else {
        return false;
    };
    tail.ends_with("/content")
}

fn extract_file_id_from_normalized(path: &str) -> Option<&str> {
    let tail = path.strip_prefix("/files/")?;
    if tail.is_empty() || tail.contains('/') {
        return None;
    }
    Some(tail)
}

pub fn normalize_path(path: &str) -> String {
    let mut out = if path.starts_with('/') {
        path.trim().to_string()
    } else {
        format!("/{}", path.trim())
    };
    while out.contains("//") {
        out = out.replace("//", "/");
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    for prefix in ["/v1", "/v1beta", "/v1beta1"] {
        if out == prefix {
            return "/".to_string();
        }
        let full = format!("{prefix}/");
        if let Some(rest) = out.strip_prefix(&full) {
            return format!("/{}", rest.trim_start_matches('/'));
        }
    }
    out
}

fn query_has_key(query: Option<&str>, key: &str) -> bool {
    query.is_some_and(|q| q.split('&').any(|pair| pair.split('=').next() == Some(key)))
}

fn query_has_value(query: Option<&str>, key: &str, value: &str) -> bool {
    query.is_some_and(|q| {
        q.split('&').any(|pair| {
            let mut it = pair.splitn(2, '=');
            it.next() == Some(key) && it.next().is_some_and(|v| v.eq_ignore_ascii_case(value))
        })
    })
}

fn stream_or_non_stream(body: &Bytes) -> OperationFamily {
    if read_stream_flag(body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    }
}

fn read_stream_flag(body: &Bytes) -> bool {
    #[derive(Deserialize)]
    struct S {
        #[serde(default)]
        stream: Option<bool>,
    }
    if body.is_empty() {
        return false;
    }
    serde_json::from_slice::<S>(body)
        .ok()
        .and_then(|v| v.stream)
        .unwrap_or(false)
}

/// Extract model name from a URI path like `/v1/models/gpt-4o` or
/// `/v1beta/models/gemini-pro:generateContent`.
pub fn extract_model_from_uri_path(path: &str) -> Option<String> {
    let normalized = normalize_path(path);
    let tail = normalized.strip_prefix("/models/")?;
    if tail.is_empty() {
        return None;
    }
    let model = tail.split(':').next().unwrap_or(tail);
    if model.is_empty() {
        return None;
    }
    Some(model.to_string())
}
