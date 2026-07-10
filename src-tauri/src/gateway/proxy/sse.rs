//! Shared SSE frame helpers for gateway proxy paths.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Find the byte offset immediately after the first complete SSE event,
/// terminated by `\n\n` or `\r\n\r\n`.
pub(in crate::gateway::proxy) fn find_sse_event_end(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buffer.len() {
        if buffer[i] == b'\n' {
            if i + 1 < buffer.len() && buffer[i + 1] == b'\n' {
                return Some(i + 2);
            }
        } else if buffer[i] == b'\r'
            && i + 3 < buffer.len()
            && buffer[i + 1] == b'\n'
            && buffer[i + 2] == b'\r'
            && buffer[i + 3] == b'\n'
        {
            return Some(i + 4);
        }
        i += 1;
    }
    None
}

pub(in crate::gateway::proxy) fn sse_frame_has_terminal_event(frame: &str) -> bool {
    let mut event_type = None;
    let mut data_parts: Vec<&str> = Vec::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            let payload = rest.trim_start();
            if payload == "[DONE]" {
                return true;
            }
            data_parts.push(payload);
        }
    }

    if event_type
        .as_deref()
        .is_some_and(is_terminal_sse_event_name)
    {
        return true;
    }

    if data_parts.is_empty() {
        return false;
    }
    let data_str = data_parts.join("\n");
    serde_json::from_str::<Value>(&data_str)
        .ok()
        .and_then(|data| {
            data.get("type")
                .and_then(Value::as_str)
                .map(is_terminal_sse_event_name)
        })
        .unwrap_or(false)
}

fn is_terminal_sse_event_name(event_name: &str) -> bool {
    matches!(
        event_name,
        "response.completed" | "response.failed" | "response.incomplete" | "error"
    )
}

/// Parse a single SSE frame string into (event_type, data_json).
///
/// Supports both `event: xxx\ndata: {...}\n\n` and `data: {...}\n\n` formats.
/// In the latter case, the event type is inferred from the `type` field of the
/// JSON data.
pub(in crate::gateway::proxy) fn parse_sse_frame(frame: &str) -> Option<(String, Value)> {
    let mut event_type = None;
    let mut data_parts: Vec<&str> = Vec::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            let payload = rest.trim_start();
            if payload == "[DONE]" {
                return None;
            }
            data_parts.push(payload);
        }
    }

    if data_parts.is_empty() {
        return None;
    }
    let data_str = data_parts.join("\n");
    let data: Value = serde_json::from_str(&data_str).ok()?;

    let event_type = event_type.unwrap_or_else(|| {
        data.get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string()
    });

    Some((event_type, data))
}

/// Aggregate an OpenAI Responses SSE stream into a single JSON response.
pub(in crate::gateway::proxy) fn aggregate_responses_event_stream(
    raw: &[u8],
) -> Result<Value, String> {
    let mut response: Option<Value> = None;
    let mut output: Vec<Value> = Vec::new();
    let mut cursor = 0usize;
    let mut saw_completed = false;
    let mut saw_done = false;

    while let Some(relative_end) = find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let text =
            std::str::from_utf8(frame).map_err(|e| format!("invalid utf-8 in SSE frame: {e}"))?;
        if sse_frame_has_done_data(text) {
            saw_done = true;
        }
        let Some((event_name, data)) = parse_sse_frame(text) else {
            continue;
        };

        match event_name.as_str() {
            "response.created" => {
                let created = data.get("response").cloned().unwrap_or(data);
                response = Some(created);
            }
            "response.output_item.done" => {
                let item = data
                    .get("item")
                    .cloned()
                    .ok_or_else(|| "missing item in response.output_item.done".to_string())?;
                upsert_output_item(&mut output, item);
            }
            "response.completed" => {
                saw_completed = true;
                let completed = data.get("response").cloned().unwrap_or(data);
                if let Some(existing) = response.as_mut() {
                    merge_response_object(existing, &completed);
                } else {
                    response = Some(completed);
                }
            }
            "response.failed" => {
                return Err(sse_error_detail(&data, "response.failed"));
            }
            "response.incomplete" => {
                return Err(sse_error_detail(&data, "response.incomplete"));
            }
            "error" => {
                return Err(sse_error_detail(&data, "unknown SSE error"));
            }
            _ => {}
        }
    }

    let mut response =
        response.ok_or_else(|| "missing response.created/response.completed".to_string())?;
    if !saw_completed {
        return if saw_done {
            Err("responses stream ended with [DONE] before response.completed".to_string())
        } else {
            Err("missing response.completed".to_string())
        };
    }
    let obj = response
        .as_object_mut()
        .ok_or_else(|| "aggregated response is not an object".to_string())?;
    obj.insert("output".to_string(), Value::Array(output));
    Ok(response)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway::proxy) struct ResponsesDeltaFinalMismatch {
    pub(in crate::gateway::proxy) delta_len_bytes: usize,
    pub(in crate::gateway::proxy) final_len_bytes: usize,
    pub(in crate::gateway::proxy) delta_sha256: String,
    pub(in crate::gateway::proxy) final_sha256: String,
    pub(in crate::gateway::proxy) final_source: &'static str,
    pub(in crate::gateway::proxy) response_id: Option<String>,
}

/// Validate that the text Codex TUI builds from visible deltas matches the
/// final visible message/refusal payload in a completed Responses SSE stream.
pub(in crate::gateway::proxy) fn validate_responses_delta_matches_final(
    raw: &[u8],
) -> Result<(), ResponsesDeltaFinalMismatch> {
    let mut delta_visible = String::new();
    let mut output_text_done_visible = String::new();
    let mut item_done_visible = String::new();
    let mut completed_visible: Option<String> = None;
    let mut response_id: Option<String> = None;
    let mut saw_visible_delta = false;
    let mut cursor = 0usize;

    while let Some(relative_end) = find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let Ok(text) = std::str::from_utf8(frame) else {
            continue;
        };
        let Some((event_name, data)) = parse_sse_frame(text) else {
            continue;
        };

        match event_name.as_str() {
            "response.output_text.delta" | "response.content_part.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        saw_visible_delta = true;
                    }
                    delta_visible.push_str(delta);
                }
            }
            "response.refusal.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        saw_visible_delta = true;
                    }
                    delta_visible.push_str(delta);
                }
            }
            "response.output_text.done" => {
                if let Some(text) = data.get("text").and_then(Value::as_str) {
                    output_text_done_visible.push_str(text);
                }
            }
            "response.refusal.done" => {
                if let Some(refusal) = data.get("refusal").and_then(Value::as_str) {
                    output_text_done_visible.push_str(refusal);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = data.get("item") {
                    append_visible_from_response_output_item(item, &mut item_done_visible);
                }
            }
            "response.completed" => {
                let response = data.get("response").unwrap_or(&data);
                if response_id.is_none() {
                    response_id = response
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if let Some(output) = response.get("output").and_then(Value::as_array) {
                    let mut visible = String::new();
                    for item in output {
                        append_visible_from_response_output_item(item, &mut visible);
                    }
                    if !visible.is_empty() {
                        completed_visible = Some(visible);
                    }
                }
            }
            _ => {}
        }
    }

    let (final_visible, final_source) = completed_visible
        .as_deref()
        .map(|value| (value, "response.completed"))
        .or_else(|| {
            (!item_done_visible.is_empty())
                .then_some((item_done_visible.as_str(), "response.output_item.done"))
        })
        .or_else(|| {
            (!output_text_done_visible.is_empty()).then_some((
                output_text_done_visible.as_str(),
                "response.output_text.done",
            ))
        })
        .unwrap_or(("", "none"));

    if delta_visible == final_visible {
        return Ok(());
    }
    if !saw_visible_delta {
        return Ok(());
    }

    Err(ResponsesDeltaFinalMismatch {
        delta_len_bytes: delta_visible.len(),
        final_len_bytes: final_visible.len(),
        delta_sha256: sha256_hex(delta_visible.as_bytes()),
        final_sha256: sha256_hex(final_visible.as_bytes()),
        final_source,
        response_id,
    })
}

fn merge_response_object(base: &mut Value, update: &Value) {
    let (Some(base_obj), Some(update_obj)) = (base.as_object_mut(), update.as_object()) else {
        *base = update.clone();
        return;
    };

    for (key, value) in update_obj {
        if key == "output" {
            continue;
        }
        base_obj.insert(key.clone(), value.clone());
    }
}

fn upsert_output_item(output: &mut Vec<Value>, item: Value) {
    let item_id = item.get("id").and_then(Value::as_str);
    if let Some(item_id) = item_id {
        if let Some(existing) = output
            .iter_mut()
            .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(item_id))
        {
            *existing = item;
            return;
        }
    }
    output.push(item);
}

fn append_visible_from_response_output_item(item: &Value, out: &mut String) {
    match item.get("type").and_then(Value::as_str) {
        Some("message") => {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for block in content {
                    append_visible_from_content_block(block, out);
                }
            }
        }
        Some("refusal") => {
            if let Some(refusal) = item.get("refusal").and_then(Value::as_str) {
                out.push_str(refusal);
            }
        }
        _ => {}
    }
}

fn append_visible_from_content_block(block: &Value, out: &mut String) {
    match block.get("type").and_then(Value::as_str) {
        Some("output_text" | "text") => {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
        Some("refusal") => {
            if let Some(refusal) = block.get("refusal").and_then(Value::as_str) {
                out.push_str(refusal);
            }
        }
        _ => {}
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn sse_frame_has_done_data(frame: &str) -> bool {
    frame.lines().any(|line| {
        line.trim_end_matches('\r')
            .strip_prefix("data:")
            .is_some_and(|payload| payload.trim_start() == "[DONE]")
    })
}

fn sse_error_detail(data: &Value, fallback: &str) -> String {
    data.get("detail")
        .and_then(Value::as_str)
        .or_else(|| data.get("message").and_then(Value::as_str))
        .or_else(|| data.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| {
            data.pointer("/response/error/message")
                .and_then(Value::as_str)
        })
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_sse_event_end_basic() {
        assert_eq!(find_sse_event_end(b"abc\n\ndef"), Some(5));
        assert_eq!(find_sse_event_end(b"abc\ndef"), None);
        assert_eq!(find_sse_event_end(b"\n\n"), Some(2));
        assert_eq!(find_sse_event_end(b"abc\r\n\r\ndef"), Some(7));
    }

    #[test]
    fn parse_sse_frame_with_event() {
        let frame = "event: response.created\ndata: {\"id\":\"r1\"}\n\n";
        let (evt, data) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "response.created");
        assert_eq!(data.get("id").unwrap().as_str().unwrap(), "r1");
    }

    #[test]
    fn parse_sse_frame_data_only_infers_type() {
        let frame = "data: {\"type\":\"response.completed\",\"id\":\"r2\"}\n\n";
        let (evt, data) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "response.completed");
        assert_eq!(data.get("id").unwrap().as_str().unwrap(), "r2");
    }

    #[test]
    fn parse_sse_frame_done_returns_none() {
        let frame = "data: [DONE]\n\n";
        assert!(parse_sse_frame(frame).is_none());
    }

    #[test]
    fn sse_frame_terminal_detection_handles_done_event_and_inferred_type() {
        assert!(sse_frame_has_terminal_event("data: [DONE]\n\n"));
        assert!(sse_frame_has_terminal_event(
            "event: response.completed\ndata: {\"response\":{\"id\":\"r1\"}}\n\n"
        ));
        assert!(sse_frame_has_terminal_event(
            "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"bad\"}}\n\n"
        ));
        assert!(sse_frame_has_terminal_event(
            "event: response.incomplete\ndata: {\"response\":{\"id\":\"r1\",\"status\":\"incomplete\"}}\n\n"
        ));
        assert!(sse_frame_has_terminal_event(
            "event: error\ndata: {\"message\":\"bad\"}\n\n"
        ));
        assert!(!sse_frame_has_terminal_event(
            ": keepalive\ndata: {\"type\":\"response.output_text.delta\"}\n\n"
        ));
    }

    #[test]
    fn parse_sse_frame_comment_lines_ignored() {
        let frame = ": keepalive\ndata: {\"type\":\"ping\"}\n\n";
        let (evt, _) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "ping");
    }

    #[test]
    fn aggregate_responses_event_stream_handles_many_frames_with_trailing_partial() {
        let mut raw = String::from(
            "event: response.created\n\
             data: {\"response\":{\"id\":\"resp_many\",\"status\":\"in_progress\"}}\n\n",
        );
        for index in 0..128 {
            raw.push_str(&format!(
                "event: response.output_item.done\n\
                 data: {{\"item\":{{\"id\":\"msg_{index}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}}}\n\n"
            ));
        }
        raw.push_str(
            "event: response.completed\n\
             data: {\"response\":{\"id\":\"resp_many\",\"status\":\"completed\"}}\n\n\
             event: response.output_item.done\n\
             data: {\"item\":{\"id\":\"partial\"",
        );

        let aggregated = aggregate_responses_event_stream(raw.as_bytes()).expect("aggregate");

        assert_eq!(aggregated["id"], "resp_many");
        assert_eq!(aggregated["status"], "completed");
        assert_eq!(
            aggregated
                .get("output")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(128)
        );
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_response_failed_terminal() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_failed\",\"status\":\"in_progress\"}}\n\n",
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",\"error\":{\"message\":\"continuation failed\"}}}\n\n",
            ": keepalive\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(err, "continuation failed");
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_response_incomplete_terminal() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_incomplete\",\"status\":\"in_progress\"}}\n\n",
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_incomplete\",\"status\":\"incomplete\"}}\n\n",
            ": keepalive\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(err, "response.incomplete");
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_done_without_completed() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_done\",\"status\":\"in_progress\"}}\n\n",
            "data: [DONE]\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(
            err,
            "responses stream ended with [DONE] before response.completed"
        );
    }

    #[test]
    fn validate_responses_delta_matches_final_accepts_long_chinese_markdown() {
        let visible = "标题\n\n- 第一项包含 [链接](https://example.com)\n- 第二项包含 `inline_code`\n- 第三项是一段较长的中文说明，用来覆盖多 bullet 和 Markdown 标点。\n";
        let visible_json = serde_json::to_string(visible).expect("visible text json");
        let raw = format!(
            concat!(
                "event: response.output_text.delta\n",
                "data: {{\"type\":\"response.output_text.delta\",\"delta\":{}}}\n\n",
                "event: response.completed\n",
                "data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_md\",\"status\":\"completed\",\"output\":[{{\"type\":\"message\",\"content\":[{{\"type\":\"output_text\",\"text\":{}}}]}}]}}}}\n\n"
            ),
            visible_json, visible_json
        );

        validate_responses_delta_matches_final(raw.as_bytes()).expect("matching stream");
    }

    #[test]
    fn validate_responses_delta_matches_final_accepts_multiple_events_in_one_chunk() {
        let raw = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello \"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"world\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello world\"}]}]}}\n\n"
        );

        validate_responses_delta_matches_final(raw.as_bytes()).expect("matching stream");
    }

    #[test]
    fn validate_responses_delta_matches_final_reports_mismatch_hashes() {
        let raw = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello \"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_bad\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello world\"}]}]}}\n\n"
        );

        let err = validate_responses_delta_matches_final(raw.as_bytes()).expect_err("mismatch");

        assert_eq!(err.delta_len_bytes, "hello ".len());
        assert_eq!(err.final_len_bytes, "hello world".len());
        assert_eq!(err.final_source, "response.completed");
        assert_eq!(err.response_id.as_deref(), Some("resp_bad"));
        assert_ne!(err.delta_sha256, err.final_sha256);
    }

    #[test]
    fn validate_responses_delta_matches_final_allows_final_only_repair_sse() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_final_only\",\"status\":\"in_progress\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final only repair\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_final_only\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"final only repair\"}]}]}}\n\n"
        );

        validate_responses_delta_matches_final(raw.as_bytes()).expect("final-only repair stream");
    }
}
