//! Usage: Guard-specific concurrent retry probes for Codex degraded reasoning.
//!
//! Probes are deliberately isolated from the main mutable loop state. The first
//! HTTP response wins and is routed through the normal response handlers; the
//! remaining probe tasks are aborted, which drops their upstream connections.

use super::attempt_executor::{AttemptSendOutcome, AttemptTiming, RetryLoopState};
use super::provider_iterator::PreparedProvider;
use super::*;
use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayRequestHookInput};
use crate::gateway::proxy::request_context::RequestContext;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;

const MAX_CONCURRENT_PROBES: u32 = 5;

pub(super) enum ConcurrentProbeOutcome {
    Winner {
        prepared: Box<PreparedProvider>,
        send_outcome: Box<AttemptSendOutcome>,
    },
    Exhausted,
}

enum ProbeAttemptOutcome {
    Send(Box<AttemptSendOutcome>),
    GuardMatched,
}

impl ProbeAttemptOutcome {
    fn send(outcome: AttemptSendOutcome) -> Self {
        Self::Send(Box::new(outcome))
    }
}

pub(super) struct GuardRetryWaveConfig {
    pub(super) retry_index: u32,
    pub(super) attempt_index_start: u32,
    pub(super) concurrency: u32,
    pub(super) interval_ms: u32,
}

pub(super) async fn run_guard_retry_wave<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &PreparedProvider,
    retry_state: &RetryLoopState,
    config: GuardRetryWaveConfig,
) -> ConcurrentProbeOutcome
where
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    let concurrency = config.concurrency.clamp(2, MAX_CONCURRENT_PROBES);
    let mut join_set = JoinSet::new();
    for lane in 0..concurrency {
        let owned_ctx = ProbeCtx::from_common(ctx);
        let input = input.clone_for_concurrent_probe();
        let mut lane_prepared = prepared.clone();
        let lane_retry_state = retry_state.clone();
        let lane_retry_index = config.retry_index.saturating_add(lane);
        let lane_attempt_index = config.attempt_index_start.saturating_add(lane);
        let delay = Duration::from_millis(config.interval_ms as u64).saturating_mul(lane);
        join_set.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let outcome = execute_probe_attempt(
                owned_ctx.as_borrowed(),
                &input,
                &mut lane_prepared,
                &lane_retry_state,
                lane_retry_index,
                lane_attempt_index,
            )
            .await;
            (lane_prepared, outcome)
        });
    }

    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((lane_prepared, ProbeAttemptOutcome::Send(send_outcome)))
                if is_winning_outcome(send_outcome.as_ref()) =>
            {
                join_set.abort_all();
                return ConcurrentProbeOutcome::Winner {
                    prepared: Box::new(lane_prepared),
                    send_outcome,
                };
            }
            Ok(_) => continue,
            Err(err) => {
                tracing::debug!("codex reasoning guard concurrent probe task failed: {err}");
            }
        }
    }

    ConcurrentProbeOutcome::Exhausted
}

struct ProbeCtx<R: tauri::Runtime = tauri::Wry> {
    state: crate::gateway::runtime::GatewayAppState<R>,
    empty_string: String,
    empty_query: Option<String>,
    empty_session_id: Option<String>,
    empty_requested_model: Option<String>,
    empty_cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings,
    empty_special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    codex_reasoning_guard_compare_mode: crate::settings::CodexReasoningGuardCompareMode,
    codex_reasoning_guard_reasoning_equals: Vec<i64>,
    codex_reasoning_guard_model_rules: Vec<crate::settings::CodexReasoningGuardModelRule>,
    empty_model_fallbacks: Vec<String>,
    empty_introspection_body: Vec<u8>,
    response_fixer_stream_config: crate::gateway::response_fixer::ResponseFixerConfig,
    response_fixer_non_stream_config: crate::gateway::response_fixer::ResponseFixerConfig,
    upstream_first_byte_timeout: Option<Duration>,
}

impl<R: tauri::Runtime> ProbeCtx<R> {
    fn from_common(ctx: CommonCtx<'_, R>) -> Self {
        Self {
            state: ctx.state.clone(),
            empty_string: String::new(),
            empty_query: None,
            empty_session_id: None,
            empty_requested_model: ctx.requested_model.clone(),
            empty_cx2cc_settings: Default::default(),
            empty_special_settings: Arc::new(Mutex::new(Vec::new())),
            codex_reasoning_guard_compare_mode: ctx.codex_reasoning_guard_compare_mode,
            codex_reasoning_guard_reasoning_equals: ctx
                .codex_reasoning_guard_reasoning_equals
                .to_vec(),
            codex_reasoning_guard_model_rules: ctx.codex_reasoning_guard_model_rules.to_vec(),
            empty_model_fallbacks: Vec::new(),
            empty_introspection_body: Vec::new(),
            response_fixer_stream_config: ctx.response_fixer_stream_config,
            response_fixer_non_stream_config: ctx.response_fixer_non_stream_config,
            upstream_first_byte_timeout: ctx.upstream_first_byte_timeout,
        }
    }

    fn as_borrowed(&self) -> CommonCtx<'_, R> {
        CommonCtx {
            state: &self.state,
            cli_key: &self.empty_string,
            forwarded_path: &self.empty_string,
            observe: false,
            method_hint: &self.empty_string,
            query: &self.empty_query,
            trace_id: &self.empty_string,
            started: Instant::now(),
            created_at_ms: 0,
            created_at: 0,
            session_id: &self.empty_session_id,
            requested_model: &self.empty_requested_model,
            cx2cc_settings: &self.empty_cx2cc_settings,
            effective_sort_mode_id: None,
            special_settings: &self.empty_special_settings,
            provider_cooldown_secs: 0,
            upstream_first_byte_timeout_secs: 0,
            upstream_first_byte_timeout: self.upstream_first_byte_timeout,
            upstream_stream_idle_timeout: None,
            upstream_request_timeout_non_streaming: None,
            verbose_provider_error: false,
            codex_reasoning_guard_enabled: true,
            codex_reasoning_guard_compare_mode: self.codex_reasoning_guard_compare_mode,
            codex_reasoning_guard_reasoning_equals: &self.codex_reasoning_guard_reasoning_equals,
            codex_reasoning_guard_model_rules: &self.codex_reasoning_guard_model_rules,
            codex_reasoning_guard_immediate_retry_budget: 0,
            codex_reasoning_guard_delayed_retry_budget: 0,
            codex_reasoning_guard_delayed_retry_ms: 0,
            codex_reasoning_guard_exhausted_action:
                crate::settings::CodexReasoningGuardExhaustedAction::ReturnError,
            codex_reasoning_guard_retry_policy:
                crate::settings::CodexReasoningGuardRetryPolicy::Single,
            codex_reasoning_guard_concurrent_max: 1,
            codex_reasoning_guard_concurrent_interval_ms: 0,
            codex_reasoning_guard_concurrent_max_attempts: 0,
            codex_reasoning_guard_model_fallbacks: &self.empty_model_fallbacks,
            enable_response_fixer: false,
            response_fixer_stream_config: self.response_fixer_stream_config,
            response_fixer_non_stream_config: self.response_fixer_non_stream_config,
            introspection_body: &self.empty_introspection_body,
        }
    }
}

async fn execute_probe_attempt<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &mut PreparedProvider,
    retry_state: &RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
) -> ProbeAttemptOutcome
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    let attempt_started_ms = input.started.elapsed().as_millis();
    let timing = AttemptTiming {
        attempt_started_ms,
        attempt_started: Instant::now(),
    };

    let url = match build_target_url(
        &prepared.provider_base_url_base,
        &prepared.upstream_forwarded_path,
        prepared.upstream_query.as_deref(),
    ) {
        Ok(url) => url,
        Err(err) => {
            return ProbeAttemptOutcome::send(AttemptSendOutcome::PluginBlocked(format!(
                "invalid upstream URL: {err}"
            )));
        }
    };

    let mut headers = input.base_headers.clone();
    ensure_cli_required_headers(&input.cli_key, &mut headers);
    if let Some((_, source_cli_key)) = prepared.bridge_source.as_ref() {
        ensure_cli_required_headers(source_cli_key, &mut headers);
    }
    codex_session_id_completion::inject_session_headers_if_needed(
        &mut headers,
        prepared.cx2cc_codex_session_id.as_deref(),
    );

    if attempt_auth::inject_auth(
        ctx,
        input,
        prepared,
        retry_state,
        &attempt_auth::AuthErrorCtx {
            attempt_index,
            retry_index,
            attempt_started_ms,
            circuit_before: &prepared.circuit_snapshot,
        },
        &mut headers,
    )
    .is_err()
    {
        return ProbeAttemptOutcome::send(AttemptSendOutcome::OAuthInjectFailed);
    }

    let clean_outcome = request_sanitizer::clean_body(input, prepared);
    let mut body_state_for_attempt = input.request_body_state.clone();
    if prepared.request_body_mutated_before_attempt
        || clean_outcome.changed()
        || clean_outcome.body != body_state_for_attempt.decoded_clone()
    {
        body_state_for_attempt.replace_decoded(clean_outcome.body.clone());
    }

    let mut semantic_headers = body_state_for_attempt.semantic_headers(&headers);
    let hook_input = GatewayRequestHookInput {
        hook_name: GatewayPluginHookName::RequestBeforeSend,
        trace_id: input.trace_id.clone(),
        cli_key: input.cli_key.clone(),
        method: input.req_method.clone(),
        path: input.forwarded_path.clone(),
        query: input.query.clone(),
        headers: semantic_headers.clone(),
        body: body_state_for_attempt.decoded_clone(),
        requested_model: input.requested_model.clone(),
    };
    match ctx.state.plugin_pipeline.run_request_hook(hook_input).await {
        Ok(output) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_audit_events(
                &ctx.state.db,
                &input.trace_id,
                output.audit_events.clone(),
            );
            if let Some(blocked) = output.blocked {
                return ProbeAttemptOutcome::send(AttemptSendOutcome::PluginBlocked(
                    blocked.reason,
                ));
            }
            semantic_headers = output.headers;
            sync_before_send_body_output(prepared, &mut body_state_for_attempt, output.body);
        }
        Err(mut err) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_error_audit_events(
                &ctx.state.db,
                &input.trace_id,
                &mut err,
            );
            return ProbeAttemptOutcome::send(AttemptSendOutcome::PluginBlocked(format!(
                "gateway plugin request hook failed: {err}"
            )));
        }
    }

    headers = semantic_headers;
    let upstream_body = body_state_for_attempt
        .finalize_for_upstream(&mut headers, crate::gateway::util::max_request_body_bytes());

    match send::send_upstream(ctx, input.req_method.clone(), url, headers, upstream_body).await {
        send::SendResult::Ok(resp) => {
            classify_probe_response(ctx, input, retry_state, resp, timing).await
        }
        send::SendResult::Timeout => ProbeAttemptOutcome::send(AttemptSendOutcome::Timeout(timing)),
        send::SendResult::Err(err) => {
            ProbeAttemptOutcome::send(AttemptSendOutcome::ReqwestError(err, timing))
        }
    }
}

async fn classify_probe_response<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    retry_state: &RetryLoopState,
    resp: reqwest::Response,
    timing: AttemptTiming,
) -> ProbeAttemptOutcome
where
    R: tauri::Runtime,
{
    let status = resp.status();
    if !status.is_success() {
        return ProbeAttemptOutcome::send(AttemptSendOutcome::Response(resp, timing));
    }

    let headers = resp.headers().clone();
    if is_event_stream(&headers) || has_non_identity_content_encoding(&headers) {
        return ProbeAttemptOutcome::send(AttemptSendOutcome::Response(resp, timing));
    }

    let provider_ttfb_ms = Some(timing.attempt_started.elapsed().as_millis());
    let body = match resp.bytes().await {
        Ok(body) => body,
        Err(err) => {
            return ProbeAttemptOutcome::send(AttemptSendOutcome::ReqwestError(err, timing))
        }
    };

    if probe_body_hits_guard(ctx, input, retry_state, body.as_ref()) {
        return ProbeAttemptOutcome::GuardMatched;
    }

    ProbeAttemptOutcome::send(AttemptSendOutcome::BufferedNonStreamResponse {
        status,
        headers,
        body,
        provider_ttfb_ms,
        timing,
    })
}

fn probe_body_hits_guard<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    retry_state: &RetryLoopState,
    body: &[u8],
) -> bool
where
    R: tauri::Runtime,
{
    probe_json_body_hits_guard(
        input.cli_key.as_str(),
        current_codex_reasoning_guard_model(input, retry_state),
        ctx.codex_reasoning_guard_compare_mode,
        ctx.codex_reasoning_guard_reasoning_equals,
        ctx.codex_reasoning_guard_model_rules,
        body,
    )
}

fn probe_json_body_hits_guard(
    cli_key: &str,
    requested_model: Option<&str>,
    compare_mode: crate::settings::CodexReasoningGuardCompareMode,
    reasoning_equals: &[i64],
    model_rules: &[crate::settings::CodexReasoningGuardModelRule],
    body: &[u8],
) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    codex_reasoning_guard::detect_from_json(
        cli_key,
        requested_model,
        &value,
        compare_mode,
        reasoning_equals,
        model_rules,
    )
    .is_some()
}

fn sync_before_send_body_output(
    prepared: &mut PreparedProvider,
    body_state_for_attempt: &mut crate::gateway::proxy::request_body::GatewayRequestBody,
    output_body: Bytes,
) {
    let previous_body = body_state_for_attempt.decoded_clone();
    body_state_for_attempt.replace_decoded(output_body.clone());
    if output_body == previous_body {
        return;
    }

    prepared.upstream_body_bytes = output_body;
    prepared.strip_request_content_encoding = true;
    prepared.request_body_mutated_before_attempt = true;
}

fn is_winning_outcome(send_outcome: &AttemptSendOutcome) -> bool {
    matches!(
        send_outcome,
        AttemptSendOutcome::Response(_, _) | AttemptSendOutcome::BufferedNonStreamResponse { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_json_body_hits_guard_for_configured_reasoning_token() {
        let body = serde_json::json!({
            "usage": {
                "output_tokens_details": {
                    "reasoning_tokens": 516
                }
            }
        });
        let encoded = serde_json::to_vec(&body).unwrap();

        assert!(probe_json_body_hits_guard(
            "codex",
            Some("gpt-5.5"),
            crate::settings::CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[],
            &encoded,
        ));
    }

    #[test]
    fn probe_json_body_allows_non_guard_reasoning_token_to_win() {
        let body = serde_json::json!({
            "usage": {
                "output_tokens_details": {
                    "reasoning_tokens": 2048
                }
            }
        });
        let encoded = serde_json::to_vec(&body).unwrap();

        assert!(!probe_json_body_hits_guard(
            "codex",
            Some("gpt-5.5"),
            crate::settings::CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[],
            &encoded,
        ));
    }

    #[test]
    fn buffered_non_stream_response_is_a_winning_probe_outcome() {
        assert!(is_winning_outcome(
            &AttemptSendOutcome::BufferedNonStreamResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: Bytes::from_static(br#"{}"#),
                provider_ttfb_ms: Some(1),
                timing: AttemptTiming {
                    attempt_started_ms: 0,
                    attempt_started: Instant::now(),
                },
            }
        ));
    }
}
