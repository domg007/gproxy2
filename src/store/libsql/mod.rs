//! Minimal libSQL/Turso **Hrana-over-HTTP** client for wasm32 targets.
//!
//! POSTs to `{url}/v2/pipeline` with `Authorization: Bearer {token}`.
//! Native code uses SeaORM directly (`store/persistence/db.rs`).
//!
//! Hrana v2 pipeline request:
//! `{"requests":[{"type":"execute","stmt":{"sql":"...","args":[...]}},{"type":"close"}]}`
//!
//! Response: `{"baton":..,"results":[{"type":"ok","response":{"type":"execute","result":{...}}},
//! {"type":"ok","response":{"type":"close"}}]}` — or `{"type":"error","error":{"message":"..."}}`.

use js_sys::{Uint8Array, global};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response, WorkerGlobalScope};

/// Errors from the libSQL HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("fetch error: {0}")]
    Fetch(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Hrana error: {0}")]
    Hrana(String),
    #[error("unexpected response shape")]
    BadResponse,
}

fn js_err(e: wasm_bindgen::JsValue) -> StoreError {
    StoreError::Fetch(format!("{e:?}"))
}

/// A single column descriptor returned by Hrana.
#[derive(Debug, Deserialize)]
pub struct Col {
    pub name: Option<String>,
}

/// Result of a single `execute` statement.
#[derive(Debug)]
pub struct QueryResult {
    pub cols: Vec<Col>,
    /// Each row is a `Vec<Value>` of Hrana typed-value objects; callers can
    /// inspect `value["type"]` / `value["value"]` for typed extraction.
    pub rows: Vec<Vec<Value>>,
    pub affected_row_count: u64,
    pub last_insert_rowid: Option<String>,
}

// ── Hrana deserialization types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct HranaResponse {
    results: Vec<HranaResult>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum HranaResult {
    Ok { response: HranaOkResponse },
    Error { error: HranaError },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum HranaOkResponse {
    Execute { result: HranaExecuteResult },
    Close,
}

#[derive(Deserialize)]
struct HranaExecuteResult {
    cols: Vec<Col>,
    rows: Vec<Vec<Value>>,
    affected_row_count: u64,
    last_insert_rowid: Option<String>,
}

#[derive(Deserialize)]
struct HranaError {
    message: String,
}

// ── Request serialization ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct Pipeline<'a> {
    requests: Vec<PipelineRequest<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum PipelineRequest<'a> {
    Execute { stmt: Stmt<'a> },
    Close,
}

#[derive(Serialize)]
struct Stmt<'a> {
    sql: &'a str,
    args: &'a [Value],
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Minimal Hrana-over-HTTP client for Turso/libSQL.
pub struct LibsqlClient {
    url: String,
    token: String,
}

impl LibsqlClient {
    /// Create a new client. `url` is the Turso database URL
    /// (e.g. `https://<db>.turso.io`); `token` is the auth token.
    pub fn new(url: String, token: String) -> Self {
        Self { url, token }
    }

    /// Execute a single SQL statement via Hrana v2 pipeline.
    pub async fn execute(&self, sql: &str, args: &[Value]) -> Result<QueryResult, StoreError> {
        let pipeline_url = format!("{}/v2/pipeline", self.url);

        let body = serde_json::to_string(&Pipeline {
            requests: vec![
                PipelineRequest::Execute {
                    stmt: Stmt { sql, args },
                },
                PipelineRequest::Close,
            ],
        })?;

        // Build fetch request.
        let js_headers = Headers::new().map_err(js_err)?;
        js_headers
            .append("Content-Type", "application/json")
            .map_err(js_err)?;
        js_headers
            .append("Authorization", &format!("Bearer {}", self.token))
            .map_err(js_err)?;

        let body_arr = {
            let bytes = body.as_bytes();
            let arr = Uint8Array::new_with_length(bytes.len() as u32);
            arr.copy_from(bytes);
            arr
        };

        let init = RequestInit::new();
        init.set_method("POST");
        init.set_headers_headers(&js_headers);
        init.set_body_opt_u8_array(Some(&body_arr));

        let js_req = Request::new_with_str_and_init(&pipeline_url, &init).map_err(js_err)?;

        let scope = global().unchecked_into::<WorkerGlobalScope>();
        let resp_val = JsFuture::from(scope.fetch_with_request(&js_req))
            .await
            .map_err(js_err)?;
        let js_resp: Response = resp_val.unchecked_into();

        let buf_promise = js_resp.array_buffer().map_err(js_err)?;
        let buf_val = JsFuture::from(buf_promise).await.map_err(js_err)?;
        let body_bytes = Uint8Array::new(&buf_val).to_vec();

        let hrana: HranaResponse = serde_json::from_slice(&body_bytes)?;

        // First result is the execute; second is close. Extract execute result.
        let mut iter = hrana.results.into_iter();
        match iter.next().ok_or(StoreError::BadResponse)? {
            HranaResult::Ok {
                response: HranaOkResponse::Execute { result },
            } => Ok(QueryResult {
                cols: result.cols,
                rows: result.rows,
                affected_row_count: result.affected_row_count,
                last_insert_rowid: result.last_insert_rowid,
            }),
            HranaResult::Error { error } => Err(StoreError::Hrana(error.message)),
            HranaResult::Ok {
                response: HranaOkResponse::Close,
            } => Err(StoreError::BadResponse),
        }
    }
}
