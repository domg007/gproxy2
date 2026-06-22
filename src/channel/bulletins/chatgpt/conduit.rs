//! ChatGPT handoff conduit (calibrated live, June 2026).
//!
//! Thinking / pro / deep-research turns answer via **`stream_handoff`**: the
//! `POST /backend-api/f/conversation` response is only a stub naming a
//! `conversation-turn-{turn_id}` topic; the real turn streams over a per-user
//! conduit WebSocket. This module drives that path:
//!
//! 1. `GET /backend-api/celsius/ws/user` → `{ websocket_url }`.
//! 2. open the WSS (rides the credential's proxy + Edge emulation via
//!    [`UpstreamClient::open_conduit`]); send a `connect` then a `subscribe` to
//!    the turn topic from `offset:"0"`.
//! 3. payload shapes (mined live, June 2026):
//!      - **thinking** turns carry arrays of `conversation-turn-stream` envelopes
//!        whose `encoded_item` is the SAME SSE-v1 delta text the inline path emits;
//!      - **consumer deep research** streams BOTH its live progress (the "CoT")
//!        and its final report as **bare-object**
//!        `conversation-update`/`update-widget-state` frames: `plan.steps[]`
//!        advance pending→in_progress→completed with a `reason` narrative
//!        ([`MsgSynth::push_widget`] → `reasoning_content`), and the finished
//!        report rides in the same frame as `widget_state.report_message` (an
//!        assistant `text` message) → the content channel.
//!      - the **o3 / API deep-research** shape instead batches whole messages via
//!        `update_type: add-messages` (`thoughts` → reasoning, assistant `text`
//!        report → final answer, deduped by id); [`MsgSynth::push_messages`]
//!        handles it.
//!
//! [`fetch_turn_stream`] yields the synthesized SSE incrementally as each frame
//! arrives (vital for multi-minute deep research), feeding the channel's
//! [`super::ChatGptStreamDecoder`].

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json};

use crate::http::client::UpstreamClient;

/// Give up if no frame arrives for this long after the last one. Deep research
/// can pause minutes between steps (a long search/browse), so this is generous.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// Hard ceiling on a single turn. Deep research routinely runs many minutes.
const TOTAL_DEADLINE_MS: u64 = 1_800_000;

/// The `connect` handshake frame the browser sends before subscribing.
const CONNECT_FRAME: &str = r#"[{"id":1,"command":{"type":"connect","presence":{"type":"presence","state":"background"}}}]"#;

/// Detect a `stream_handoff` in the `/f/conversation` stub and pull the
/// `turn_exchange_id` (the conduit topic suffix). Returns `None` when the
/// response streamed inline (no handoff) and the caller should use it directly.
pub(super) fn extract_handoff_turn(stub_sse: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stub_sse).ok()?;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim_start();
        // cheap pre-filter before parsing
        if !data.contains("stream_handoff") {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(data)
            && v.get("type").and_then(Value::as_str) == Some("stream_handoff")
            && let Some(turn) = v.get("turn_exchange_id").and_then(Value::as_str)
        {
            return Some(turn.to_string());
        }
    }
    None
}

/// Streaming variant of the conduit reader: connect + subscribe, then return a
/// byte stream that yields synthesized SSE-v1 deltas AS each conduit frame
/// arrives — so the client sees thinking / deep-research output incrementally
/// instead of after the whole (possibly multi-minute) turn. The channel's
/// `stream_decoder` consumes the yielded SSE and emits OpenAI chunks + `[DONE]`.
pub(super) async fn fetch_turn_stream(
    client: Arc<dyn UpstreamClient>,
    secret: Value,
    base: String,
    turn_id: String,
) -> Result<crate::http::client::RespStream, String> {
    use futures_util::StreamExt;

    let ws_url = conduit_url(&client, &secret, &base).await?;
    let mut sock = client
        .open_conduit(&ws_url)
        .await
        .map_err(|e| format!("open conduit: {e}"))?;
    sock.send_text(CONNECT_FRAME.to_string())
        .await
        .map_err(|e| format!("conduit connect: {e}"))?;
    let subscribe = format!(
        r#"[{{"id":2,"command":{{"type":"subscribe","topic_id":"conversation-turn-{turn_id}","offset":"0"}}}}]"#
    );
    sock.send_text(subscribe)
        .await
        .map_err(|e| format!("conduit subscribe: {e}"))?;

    let deadline_ms = crate::util::time::unix_now_ms() + TOTAL_DEADLINE_MS;
    let stream =
        futures_util::stream::unfold(Some((sock, MsgSynth::default())), move |state| async move {
            let (mut sock, mut synth) = state?;
            loop {
                if crate::util::time::unix_now_ms() >= deadline_ms {
                    return None;
                }
                match tokio::time::timeout(IDLE_TIMEOUT, sock.recv_text()).await {
                    Ok(Some(Ok(frame))) => {
                        let mut out = String::new();
                        let done = absorb_frame(&frame, &mut out, &mut synth);
                        if !out.is_empty() {
                            let next = if done { None } else { Some((sock, synth)) };
                            return Some((Ok(Bytes::from(out)), next));
                        }
                        if done {
                            return None;
                        }
                        // No output this frame (heartbeat / noise) — keep reading.
                    }
                    // socket closed, idle timeout, or recv error → end the stream
                    _ => return None,
                }
            }
        });
    Ok(stream.boxed())
}

/// Fetch the per-user conduit `websocket_url`.
async fn conduit_url(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
) -> Result<String, String> {
    let url = format!("{base}/backend-api/celsius/ws/user");
    let mut req = http::Request::get(url)
        .body(Bytes::new())
        .map_err(|e| format!("conduit url request: {e}"))?;
    super::auth::apply_request_headers(&mut req, secret).map_err(|e| e.to_string())?;
    let resp = client
        .send(req)
        .await
        .map_err(|e| format!("conduit url send: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("celsius/ws/user: {}", resp.status()));
    }
    let v: Value =
        serde_json::from_slice(resp.body()).map_err(|e| format!("conduit url parse: {e}"))?;
    v.get("websocket_url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "celsius/ws/user: missing websocket_url".into())
}

/// Absorb one received frame (a JSON array of pub/sub envelopes). Thinking turns
/// carry `encoded_item` SSE deltas (appended verbatim); deep-research turns carry
/// `update_type: add-messages` whole-message batches (synthesized into SSE by
/// `synth`). Returns `true` when the turn is complete.
fn absorb_frame(frame: &str, sse: &mut String, synth: &mut MsgSynth) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return false;
    };
    // Thinking turns send arrays of pub/sub envelopes; consumer deep-research
    // `conversation-update` frames arrive as a single bare object. Accept both.
    let envelopes: Vec<&Value> = match &value {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![&value],
        _ => return false,
    };
    let mut done = false;
    for env in envelopes {
        // Explicit turn-complete envelope.
        if env.pointer("/payload/type").and_then(Value::as_str)
            == Some("conversation-turn-complete")
        {
            done = true;
        }
        // Deep research: a `conversation-update` with `add-messages` (the
        // update object sits at `payload` or, when wrapped, `payload.payload`).
        for cu in [env.get("payload"), env.pointer("/payload/payload")]
            .into_iter()
            .flatten()
        {
            match cu.get("update_type").and_then(Value::as_str) {
                Some("add-messages") => {
                    if let Some(msgs) = cu
                        .pointer("/update_content/messages")
                        .and_then(Value::as_array)
                    {
                        done |= synth.push_messages(msgs, sse);
                    }
                }
                // Consumer deep research streams its live PROGRESS (the "CoT")
                // AND its final report as `update-widget-state` plan widgets, not
                // `thoughts`/`add-messages`.
                Some("update-widget-state") => {
                    if let Some(uc) = cu.get("update_content") {
                        done |= synth.push_widget(uc, sse);
                    }
                }
                _ => {}
            }
        }
        // Thinking: subscribe-reply catchups replay the turn so far.
        if let Some(catchups) = env.pointer("/reply/catchups").and_then(Value::as_array) {
            for c in catchups {
                if let Some(item) = c
                    .pointer("/payload/payload/encoded_item")
                    .and_then(Value::as_str)
                {
                    done |= push_item(item, sse);
                }
            }
        }
        // Thinking: live stream item.
        if let Some(item) = env
            .pointer("/payload/payload/encoded_item")
            .and_then(Value::as_str)
        {
            done |= push_item(item, sse);
        }
    }
    done
}

/// Append one `encoded_item` SSE fragment; returns `true` if it marks the end of
/// the turn (`message_stream_complete` typed event, or the SSE `[DONE]`).
fn push_item(item: &str, sse: &mut String) -> bool {
    sse.push_str(item);
    if !item.ends_with('\n') {
        sse.push('\n');
    }
    item.contains("message_stream_complete") || item.contains("[DONE]")
}

/// Synthesizes SSE-v1 deltas from deep-research `add-messages` whole-message
/// batches, deduplicating by message id so the report's in-place snapshot growth
/// emits only the NEW text (no duplication). `thoughts` messages → reasoning
/// channel adds (the decoder surfaces each as `reasoning_content`); the assistant
/// `text` report → one accumulating final-answer channel. Internal tool/code
/// messages are skipped. Channels are explicitly numbered so the decoder routes
/// correctly even when thoughts and report interleave.
#[derive(Default)]
pub(super) struct MsgSynth {
    next_ch: u64,
    report_ch: Option<u64>,
    report_text: String,
    thoughts_emitted: std::collections::HashMap<String, usize>,
    /// Per-plan-step signature (`status|reason`) last surfaced, so widget-state
    /// replays don't re-emit unchanged steps.
    plan_seen: std::collections::HashMap<String, String>,
    banner: bool,
}

impl MsgSynth {
    /// Process one whole-message batch, appending synthesized SSE to `out`.
    /// Returns `true` when the report message reached `finished_successfully`.
    pub(super) fn push_messages(&mut self, messages: &[Value], out: &mut String) -> bool {
        self.ensure_banner(out);
        let mut done = false;
        for m in messages {
            let role = m.pointer("/author/role").and_then(Value::as_str);
            let content = m.get("content");
            let ct = content
                .and_then(|c| c.get("content_type"))
                .and_then(Value::as_str);
            let recipient = m.get("recipient").and_then(Value::as_str).unwrap_or("all");
            match ct {
                Some("thoughts") => {
                    let id = m.get("id").and_then(Value::as_str).unwrap_or("");
                    let Some(arr) = content
                        .and_then(|c| c.get("thoughts"))
                        .and_then(Value::as_array)
                    else {
                        continue;
                    };
                    let emitted = self.thoughts_emitted.get(id).copied().unwrap_or(0);
                    if arr.len() > emitted {
                        let ch = self.alloc_ch();
                        let add = json!({"v":{"message":{"author":{"role":"assistant"},
                            "content":{"content_type":"thoughts","thoughts":arr[emitted..]},
                            "status":"in_progress"}},"c":ch});
                        sse_event(out, &add);
                        self.thoughts_emitted.insert(id.to_string(), arr.len());
                    }
                }
                Some("text") if role == Some("assistant") && recipient == "all" => {
                    let text = content
                        .and_then(|c| c.pointer("/parts/0"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.emit_report(text, out);
                    if m.get("status").and_then(Value::as_str) == Some("finished_successfully") {
                        done = true;
                    }
                }
                _ => {} // tool / code / system — internal, not surfaced
            }
        }
        done
    }

    /// Emit the SSE `delta_encoding` banner once, before the first delta.
    fn ensure_banner(&mut self, out: &mut String) {
        if !self.banner {
            out.push_str("event: delta_encoding\ndata: \"v1\"\n\n");
            self.banner = true;
        }
    }

    /// Append the assistant report text on the (single) content channel, emitting
    /// only the growth beyond what was already sent (the report arrives as a
    /// snapshot that grows / re-sends in place, so naive appends would duplicate).
    fn emit_report(&mut self, text: &str, out: &mut String) {
        let ch = match self.report_ch {
            Some(c) => c,
            None => {
                let c = self.alloc_ch();
                self.report_ch = Some(c);
                let add = json!({"v":{"message":{"author":{"role":"assistant"},
                    "content":{"content_type":"text","parts":[""]},
                    "status":"in_progress","metadata":{"model_slug":"gpt-5"}}},"c":c});
                sse_event(out, &add);
                c
            }
        };
        let delta = if text.starts_with(&self.report_text) {
            &text[self.report_text.len()..]
        } else {
            text
        };
        if !delta.is_empty() {
            sse_event(
                out,
                &json!({"p":"/message/content/parts/0","o":"append","v":delta,"c":ch}),
            );
        }
        self.report_text = text.to_string();
    }

    fn alloc_ch(&mut self) -> u64 {
        let c = self.next_ch;
        self.next_ch += 1;
        c
    }

    /// Surface deep-research live progress from an `update-widget-state` plan
    /// widget. Each `plan.steps[]` entry advances pending→in_progress→completed
    /// and gains a `reason` narrative; emit each *changed* non-pending step as a
    /// reasoning `thoughts` entry (summary = step text, content = reason), so the
    /// research progress streams as `reasoning_content`. Deduped by step id +
    /// `status|reason` signature, since the widget replays the whole plan on every
    /// update. The final REPORT rides in the SAME widget frame as
    /// `widget_state.report_message` (an assistant `text` message), so emit it on
    /// the content channel and complete the turn when it's `finished_successfully`.
    /// Returns `true` when the report is final.
    pub(super) fn push_widget(&mut self, update_content: &Value, out: &mut String) -> bool {
        let Some(updates) = update_content.get("updates").and_then(Value::as_array) else {
            return false;
        };
        let mut done = false;
        for upd in updates {
            let ws = upd.get("widget_state");
            // (a) live progress: plan steps → reasoning_content
            if let Some(steps) = ws
                .and_then(|w| w.pointer("/plan/steps"))
                .and_then(Value::as_array)
            {
                for step in steps {
                    self.push_step(step, out);
                }
            }
            // (b) final report: `report_message` (assistant text) → content
            if let Some(rm) = ws.and_then(|w| w.get("report_message")) {
                if let Some(text) = rm.pointer("/content/parts/0").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    self.ensure_banner(out);
                    self.emit_report(text, out);
                }
                if rm.get("status").and_then(Value::as_str) == Some("finished_successfully") {
                    done = true;
                }
            }
        }
        done
    }

    /// Emit one plan step as a reasoning thought, deduped by `id+status|reason`.
    fn push_step(&mut self, step: &Value, out: &mut String) {
        let status = step.get("status").and_then(Value::as_str).unwrap_or("");
        if status.is_empty() || status == "pending" {
            return; // nothing meaningful to narrate yet
        }
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let text = step.get("text").and_then(Value::as_str).unwrap_or("");
        let reason = step.get("reason").and_then(Value::as_str).unwrap_or("");
        let sig = format!("{status}|{reason}");
        if self.plan_seen.get(id) == Some(&sig) {
            return; // unchanged since last surfaced
        }
        self.plan_seen.insert(id.to_string(), sig);
        self.ensure_banner(out);
        let marker = if status == "completed" { "✓" } else { "…" };
        let summary = format!("{marker} {text}");
        let content = if reason.is_empty() {
            status.to_string()
        } else {
            reason.to_string()
        };
        let ch = self.alloc_ch();
        let add = json!({"v":{"message":{"author":{"role":"assistant"},
            "content":{"content_type":"thoughts","thoughts":[{"summary":summary,"content":content}]},
            "status":"in_progress"}},"c":ch});
        sse_event(out, &add);
    }
}

/// Emit one `event: delta` SSE record carrying `data`.
fn sse_event(out: &mut String, data: &Value) {
    out.push_str("event: delta\ndata: ");
    out.push_str(&data.to_string());
    out.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_handoff_turn_id() {
        let stub = b"event: delta_encoding\ndata: \"v1\"\n\ndata: {\"type\": \"stream_handoff\", \"conversation_id\": \"c1\", \"turn_exchange_id\": \"turn-abc\", \"options\": []}\n\ndata: [DONE]\n\n";
        assert_eq!(extract_handoff_turn(stub).as_deref(), Some("turn-abc"));
    }

    #[test]
    fn no_handoff_in_inline_stream() {
        let inline = b"event: delta\ndata: {\"v\":{\"message\":{\"author\":{\"role\":\"assistant\"},\"content\":{\"content_type\":\"text\",\"parts\":[\"\"]}}},\"c\":2}\n\n";
        assert_eq!(extract_handoff_turn(inline), None);
    }

    #[test]
    fn absorbs_stream_items_and_detects_complete() {
        let mut sse = String::new();
        let mut synth = MsgSynth::default();
        // a live stream-item envelope carrying SSE text
        let f1 = r#"[{"type":"message","topic_id":"conversation-turn-x","payload":{"type":"conversation-turn-stream","payload":{"type":"stream-item","encoded_item":"event: delta\ndata: {\"v\":\"hi\"}\n\n"}}}]"#;
        assert!(!absorb_frame(f1, &mut sse, &mut synth));
        assert!(sse.contains("\"v\":\"hi\""));
        // completion via message_stream_complete inside an encoded_item
        let f2 = r#"[{"type":"message","topic_id":"conversation-turn-x","payload":{"type":"conversation-turn-stream","payload":{"type":"stream-item","encoded_item":"data: {\"type\": \"message_stream_complete\"}\n\n"}}}]"#;
        assert!(absorb_frame(f2, &mut sse, &mut MsgSynth::default()));
    }

    #[test]
    fn absorbs_catchups_from_subscribe_reply() {
        let mut sse = String::new();
        let mut synth = MsgSynth::default();
        let f = r#"[{"id":2,"type":"reply","reply":{"type":"subscribe","topic_id":"conversation-turn-x","catchups":[{"type":"message","payload":{"type":"conversation-turn-stream","payload":{"type":"stream-item","encoded_item":"data: {\"v\":\"replayed\"}\n\n"}}}]}}]"#;
        assert!(!absorb_frame(f, &mut sse, &mut synth));
        assert!(sse.contains("replayed"));
    }

    #[test]
    fn synthesizes_deep_research_add_messages() {
        // Deep research carries `add-messages` whole messages. Two thoughts
        // (CoT steps) then the report growing in place (snapshot). The synthesized
        // SSE, run through the real decoder, must yield the CoT as
        // `reasoning_content` and the report as `content` (snapshot growth NOT
        // duplicated).
        let mut sse = String::new();
        let mut synth = MsgSynth::default();
        let f1 = r#"[{"type":"message","payload":{"type":"conversation-update","update_type":"add-messages","update_content":{"messages":[{"id":"t1","author":{"role":"assistant"},"recipient":"all","content":{"content_type":"thoughts","thoughts":[{"summary":"Searching","content":"look up tokio releases"}]}}]}}}]"#;
        let f2 = r#"[{"type":"message","payload":{"type":"conversation-update","update_type":"add-messages","update_content":{"messages":[{"id":"a1","author":{"role":"assistant"},"recipient":"all","content":{"content_type":"text","parts":["报告:tokio "]},"status":"in_progress"}]}}}]"#;
        // snapshot growth of the SAME report message id
        let f3 = r#"[{"type":"message","payload":{"type":"conversation-update","update_type":"add-messages","update_content":{"messages":[{"id":"a1","author":{"role":"assistant"},"recipient":"all","content":{"content_type":"text","parts":["报告:tokio 1.0 于 2020 发布。"]},"status":"finished_successfully"}]}}}]"#;
        absorb_frame(f1, &mut sse, &mut synth);
        absorb_frame(f2, &mut sse, &mut synth);
        let done = absorb_frame(f3, &mut sse, &mut synth);
        assert!(done, "finished_successfully should complete the turn");

        // Decode the synthesized SSE through the real channel decoder.
        let chunks =
            crate::channel::bulletins::chatgpt::sse_to_openai::collect_all("gpt-5", sse.as_bytes());
        let reasoning: String = chunks
            .iter()
            .flat_map(|c| c.choices.iter())
            .filter_map(|ch| ch.delta.get("reasoning_content").and_then(Value::as_str))
            .collect();
        let content: String = chunks
            .iter()
            .flat_map(|c| c.choices.iter())
            .filter_map(|ch| ch.delta.get("content").and_then(Value::as_str))
            .collect();
        assert!(
            reasoning.contains("look up tokio releases"),
            "CoT not surfaced: {reasoning}"
        );
        // Report assembled once, snapshot growth not duplicated.
        assert_eq!(content, "报告:tokio 1.0 于 2020 发布。");
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;

    /// Consumer deep research streams progress as a single bare-object
    /// `conversation-update`/`update-widget-state` carrying a `plan.steps[]`
    /// list (NOT an array of envelopes, NOT `thoughts` messages). Each non-pending
    /// step must surface as `reasoning_content`, and replays of unchanged steps
    /// must NOT duplicate. Shapes mined byte-for-byte from a real live run.
    #[test]
    fn decodes_deep_research_widget_progress() {
        let mut sse = String::new();
        let mut synth = MsgSynth::default();
        // v1: step-1 completed (with reason), step-2 in_progress, step-3 pending.
        let f1 = r#"{"type":"conversation-update","payload":{"conversation_id":"c","update_type":"update-widget-state","update_content":{"widget_session_id":"w","updates":[{"message_id":"m","widget_state":{"plan":{"plan_id":"p","version":1,"steps":[{"id":"step-1","text":"检索 serde 官方仓库","status":"completed","reason":"已检索 releases/tags"},{"id":"step-2","text":"查找 0.x 到 1.0 版本","status":"in_progress","reason":"正在定位 1.0"},{"id":"step-3","text":"定位 derive 宏","status":"pending","reason":null}]},"status":"researching"}}]}}}"#;
        // v2: step-1 unchanged (must NOT re-emit), step-2 now completed (new state).
        let f2 = r#"{"type":"conversation-update","payload":{"conversation_id":"c","update_type":"update-widget-state","update_content":{"widget_session_id":"w","updates":[{"message_id":"m","widget_state":{"plan":{"plan_id":"p","version":2,"steps":[{"id":"step-1","text":"检索 serde 官方仓库","status":"completed","reason":"已检索 releases/tags"},{"id":"step-2","text":"查找 0.x 到 1.0 版本","status":"completed","reason":"确认 1.0.0 为 2017-04"},{"id":"step-3","text":"定位 derive 宏","status":"pending","reason":null}]},"status":"researching"}}]}}}"#;
        assert!(!absorb_frame(f1, &mut sse, &mut synth));
        assert!(!absorb_frame(f2, &mut sse, &mut synth));
        // v3: research complete — the final report rides in `report_message`
        // (assistant text, `finished_successfully`) in the SAME widget frame.
        let f3 = r##"{"type":"conversation-update","payload":{"conversation_id":"c","update_type":"update-widget-state","update_content":{"updates":[{"message_id":"m","widget_state":{"status":"completed","report_message":{"id":"r1","author":{"role":"assistant","metadata":{"real_author":"tool:web"}},"recipient":"all","content":{"content_type":"text","parts":["# Serde 报告\n\n执行摘要:serde_derive 0.8.6。"]},"status":"finished_successfully"}}}]}}}"##;
        assert!(
            absorb_frame(f3, &mut sse, &mut synth),
            "report finished → done"
        );

        let chunks =
            crate::channel::bulletins::chatgpt::sse_to_openai::collect_all("gpt-5", sse.as_bytes());
        let reasoning: String = chunks
            .iter()
            .flat_map(|c| c.choices.iter())
            .filter_map(|ch| ch.delta.get("reasoning_content").and_then(Value::as_str))
            .collect();
        let content: String = chunks
            .iter()
            .flat_map(|c| c.choices.iter())
            .filter_map(|ch| ch.delta.get("content").and_then(Value::as_str))
            .collect();
        // step-1 + step-2(in_progress) + step-2(completed) surfaced; pending skipped.
        assert!(
            reasoning.contains("检索 serde 官方仓库"),
            "step-1: {reasoning}"
        );
        assert!(
            reasoning.contains("正在定位 1.0"),
            "step-2 in_progress: {reasoning}"
        );
        assert!(
            reasoning.contains("确认 1.0.0 为 2017-04"),
            "step-2 completed: {reasoning}"
        );
        assert!(
            !reasoning.contains("定位 derive 宏"),
            "pending leaked: {reasoning}"
        );
        // step-1's reason appears exactly once despite the v2 replay (dedup).
        assert_eq!(reasoning.matches("已检索 releases/tags").count(), 1);
        // the report surfaces as `content` (the final answer), not reasoning.
        assert!(
            content.contains("执行摘要:serde_derive 0.8.6"),
            "report not surfaced as content: {content}"
        );
    }
}
