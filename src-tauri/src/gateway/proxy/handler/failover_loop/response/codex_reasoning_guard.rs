//! Usage: Codex degraded-reasoning detection helpers.

use crate::gateway::events::{decision_chain as dc, FailoverAttempt};
use crate::gateway::proxy::ErrorCategory;
use crate::gateway::response_fixer;
use crate::settings::{
    CodexReasoningGuardCompareMode, CodexReasoningGuardExhaustedAction,
    CodexReasoningGuardModelRule, CodexReasoningGuardRetryPolicy,
};
use axum::http::StatusCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) const CODEX_REASONING_GUARD_ERROR_CODE: &str = "GW_CODEX_REASONING_GUARD";
pub(super) const CODEX_REASONING_GUARD_REASON_CODE: &str = "codex_reasoning_guard";
const CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT: &str = "global_default";
const CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE: &str = "model_rule";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardMatch {
    pub(super) reasoning_tokens: i64,
    pub(super) pointer: &'static str,
    pub(super) compare_mode: CodexReasoningGuardCompareMode,
    pub(super) matched_rule_value: i64,
    pub(super) requested_model: Option<String>,
    pub(super) rule_source: &'static str,
    pub(super) rule_model: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedCodexReasoningGuardRule<'a> {
    compare_mode: CodexReasoningGuardCompareMode,
    configured_values: &'a [i64],
    rule_source: &'static str,
    rule_model: Option<&'a str>,
}

const REASONING_POINTERS: &[&str] = &[
    "/usage/output_tokens_details/reasoning_tokens",
    "/usage/completion_tokens_details/reasoning_tokens",
    "/response/usage/output_tokens_details/reasoning_tokens",
    "/response/usage/completion_tokens_details/reasoning_tokens",
];

pub(super) fn detect_from_json(
    cli_key: &str,
    requested_model: Option<&str>,
    value: &serde_json::Value,
    fallback_compare_mode: CodexReasoningGuardCompareMode,
    fallback_values: &[i64],
    model_rules: &[CodexReasoningGuardModelRule],
) -> Option<CodexReasoningGuardMatch> {
    if cli_key != "codex" {
        return None;
    }
    let resolved_rule = resolve_guard_rule(
        requested_model,
        fallback_compare_mode,
        fallback_values,
        model_rules,
    )?;

    for pointer in REASONING_POINTERS {
        let Some(raw) = value.pointer(pointer) else {
            continue;
        };
        let reasoning_tokens = match raw {
            serde_json::Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|v| i64::try_from(v).ok())),
            _ => None,
        };
        let Some(reasoning_tokens) = reasoning_tokens else {
            continue;
        };
        if let Some(matched_rule_value) = find_matched_rule_value(
            resolved_rule.compare_mode,
            reasoning_tokens,
            resolved_rule.configured_values,
        ) {
            return Some(CodexReasoningGuardMatch {
                reasoning_tokens,
                pointer,
                compare_mode: resolved_rule.compare_mode,
                matched_rule_value,
                requested_model: requested_model
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(ToOwned::to_owned),
                rule_source: resolved_rule.rule_source,
                rule_model: resolved_rule.rule_model.map(ToOwned::to_owned),
            });
        }
    }

    None
}

fn resolve_guard_rule<'a>(
    requested_model: Option<&str>,
    fallback_compare_mode: CodexReasoningGuardCompareMode,
    fallback_values: &'a [i64],
    model_rules: &'a [CodexReasoningGuardModelRule],
) -> Option<ResolvedCodexReasoningGuardRule<'a>> {
    let requested_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(requested_model) = requested_model {
        if let Some(rule) = model_rules
            .iter()
            .find(|rule| rule.requested_model == requested_model)
        {
            if !rule.reasoning_equals.is_empty() {
                return Some(ResolvedCodexReasoningGuardRule {
                    compare_mode: rule.compare_mode,
                    configured_values: &rule.reasoning_equals,
                    rule_source: CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE,
                    rule_model: Some(rule.requested_model.as_str()),
                });
            }
        }
    }
    if fallback_values.is_empty() {
        return None;
    }
    Some(ResolvedCodexReasoningGuardRule {
        compare_mode: fallback_compare_mode,
        configured_values: fallback_values,
        rule_source: CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT,
        rule_model: None,
    })
}

fn find_matched_rule_value(
    compare_mode: CodexReasoningGuardCompareMode,
    reasoning_tokens: i64,
    configured_values: &[i64],
) -> Option<i64> {
    match compare_mode {
        CodexReasoningGuardCompareMode::Equals => configured_values
            .iter()
            .copied()
            .find(|value| *value == reasoning_tokens),
        CodexReasoningGuardCompareMode::LessThanOrEqual => configured_values
            .iter()
            .copied()
            .filter(|value| reasoning_tokens <= *value)
            .min(),
    }
}

fn compare_mode_symbol(compare_mode: CodexReasoningGuardCompareMode) -> &'static str {
    match compare_mode {
        CodexReasoningGuardCompareMode::Equals => "==",
        CodexReasoningGuardCompareMode::LessThanOrEqual => "<=",
    }
}

pub(super) fn push_special_setting(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name: &str,
    retry_index: u32,
    matched: &CodexReasoningGuardMatch,
    budget: CodexReasoningGuardBudgetDecision,
) {
    response_fixer::push_special_setting(
        special_settings,
        serde_json::json!({
            "type": "codex_reasoning_guard",
            "scope": "attempt",
            "hit": true,
            "providerId": provider_id,
            "providerName": provider_name,
            "reasoningTokens": matched.reasoning_tokens,
            "compareMode": matched.compare_mode,
            "compareModeSymbol": compare_mode_symbol(matched.compare_mode),
            "matchedRuleValue": matched.matched_rule_value,
            "pointer": matched.pointer,
            "requestedModel": matched.requested_model,
            "ruleSource": matched.rule_source,
            "ruleModel": matched.rule_model,
            "retryAttemptNumber": retry_index,
            "retryAttemptNumberNext": retry_index.saturating_add(1),
            "displayStatus": StatusCode::BAD_GATEWAY.as_u16(),
            "action": budget.action_taken,
            "actionTaken": budget.action_taken,
            "backoffApplied": budget.delay_ms > 0,
            "backoffAfterHits": budget.immediate_budget,
            "backoffMs": budget.delay_ms,
            "guardHitNumber": budget.hit_number,
            "guardRetryPhase": budget.phase,
            "guardBudgetRemaining": budget.remaining_budget,
            "guardBudgetTotal": budget.total_budget,
            "guardExhaustedAction": budget.exhausted_action,
            "guardRetryPolicy": budget.retry_wave.map(|wave| match wave.policy {
                CodexReasoningGuardRetryPolicy::Single => "single",
                CodexReasoningGuardRetryPolicy::Concurrent => "concurrent",
            }),
            "guardRetryConcurrency": budget.retry_wave.map(|wave| wave.concurrency),
            "guardRetryIntervalMs": budget.retry_wave.map(|wave| wave.interval_ms),
            "guardRetryMaxAttempts": budget.retry_wave.map(|wave| wave.max_attempts),
            "guardRetryWaveExhausted": budget.retry_wave.map(|wave| wave.exhausted),
        }),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexReasoningGuardBudgetAction {
    RetrySameProvider,
    ReturnError,
    SwitchProvider,
    SwitchModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardBudgetDecision {
    pub(super) action: CodexReasoningGuardBudgetAction,
    pub(super) hit_number: u32,
    pub(super) phase: &'static str,
    pub(super) delay_ms: u32,
    pub(super) retry_wave: Option<CodexReasoningGuardRetryWave>,
    pub(super) immediate_budget: u32,
    pub(super) delayed_budget: u32,
    pub(super) total_budget: u32,
    pub(super) remaining_budget: u32,
    pub(super) exhausted_action: &'static str,
    pub(super) action_taken: &'static str,
}

pub(super) fn budget_decision(
    current_hits: u32,
    immediate_budget: u32,
    delayed_budget: u32,
    delayed_retry_ms: u32,
    exhausted_action: CodexReasoningGuardExhaustedAction,
    retry_policy: CodexReasoningGuardRetryPolicy,
    concurrent_max: u32,
    concurrent_interval_ms: u32,
    concurrent_max_attempts: u32,
) -> CodexReasoningGuardBudgetDecision {
    let hit_number = current_hits.saturating_add(1);
    let total_budget = immediate_budget.saturating_add(delayed_budget);
    let wave = retry_wave(
        current_hits,
        retry_policy,
        concurrent_max,
        concurrent_interval_ms,
        concurrent_max_attempts,
    );
    let exhausted_action_label = match exhausted_action {
        CodexReasoningGuardExhaustedAction::ReturnError => "return_error",
        CodexReasoningGuardExhaustedAction::SwitchProvider => "switch_provider",
        CodexReasoningGuardExhaustedAction::SwitchModel => "switch_model",
    };

    if hit_number <= immediate_budget && !wave.exhausted {
        return CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::RetrySameProvider,
            hit_number,
            phase: "immediate",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget,
            delayed_budget,
            total_budget,
            remaining_budget: total_budget.saturating_sub(hit_number),
            exhausted_action: exhausted_action_label,
            action_taken: "retry_same_provider_no_circuit",
        };
    }

    if hit_number <= total_budget && !wave.exhausted {
        return CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::RetrySameProvider,
            hit_number,
            phase: "delayed",
            delay_ms: delayed_retry_ms,
            retry_wave: Some(wave),
            immediate_budget,
            delayed_budget,
            total_budget,
            remaining_budget: total_budget.saturating_sub(hit_number),
            exhausted_action: exhausted_action_label,
            action_taken: "retry_same_provider_delayed_no_circuit",
        };
    }

    match exhausted_action {
        CodexReasoningGuardExhaustedAction::ReturnError => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::ReturnError,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget,
            delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label,
            action_taken: "return_guard_error_no_circuit",
        },
        CodexReasoningGuardExhaustedAction::SwitchProvider => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::SwitchProvider,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget,
            delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label,
            action_taken: "switch_provider_no_circuit",
        },
        CodexReasoningGuardExhaustedAction::SwitchModel => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::SwitchModel,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget,
            delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label,
            action_taken: "switch_model_no_circuit",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardRetryWave {
    pub(super) policy: CodexReasoningGuardRetryPolicy,
    pub(super) hit_number: u32,
    pub(super) concurrency: u32,
    pub(super) interval_ms: u32,
    pub(super) max_attempts: u32,
    pub(super) exhausted: bool,
}

pub(super) fn retry_wave(
    current_hits: u32,
    policy: CodexReasoningGuardRetryPolicy,
    concurrent_max: u32,
    concurrent_interval_ms: u32,
    concurrent_max_attempts: u32,
) -> CodexReasoningGuardRetryWave {
    let hit_number = current_hits.saturating_add(1);
    let max_attempts = concurrent_max_attempts;
    let exhausted = max_attempts > 0 && hit_number > max_attempts;
    let concurrency = match policy {
        CodexReasoningGuardRetryPolicy::Single => 1,
        CodexReasoningGuardRetryPolicy::Concurrent => hit_number.min(concurrent_max.max(1)).max(1),
    };

    CodexReasoningGuardRetryWave {
        policy,
        hit_number,
        concurrency,
        interval_ms: concurrent_interval_ms,
        max_attempts,
        exhausted,
    }
}

pub(super) fn select_next_model_fallback<'a>(
    current_model: Option<&str>,
    fallbacks: &'a [String],
) -> Option<&'a str> {
    let current = current_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    fallbacks
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .find(|model| Some(*model) != current)
}

pub(super) async fn apply_delay_if_needed(decision: CodexReasoningGuardBudgetDecision) {
    if decision.delay_ms == 0 {
        return;
    }
    tokio::time::sleep(Duration::from_millis(decision.delay_ms as u64)).await;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record_guard_retry_attempt(
    attempts: &mut Vec<FailoverAttempt>,
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    provider_index: u32,
    retry_index: u32,
    session_reuse: Option<bool>,
    attempt_started_ms: u128,
    attempt_duration_ms: u128,
    circuit_state_before: &'static str,
    circuit_failure_count: u32,
    circuit_failure_threshold: u32,
    matched: &CodexReasoningGuardMatch,
    budget: CodexReasoningGuardBudgetDecision,
) {
    let (outcome, decision) = match budget.action {
        CodexReasoningGuardBudgetAction::RetrySameProvider => {
            ("codex_reasoning_guard_retry", "retry_same_provider")
        }
        CodexReasoningGuardBudgetAction::ReturnError => {
            ("codex_reasoning_guard_exhausted", "abort")
        }
        CodexReasoningGuardBudgetAction::SwitchProvider => {
            ("codex_reasoning_guard_switch_provider", "switch")
        }
        CodexReasoningGuardBudgetAction::SwitchModel => {
            ("codex_reasoning_guard_switch_model", "retry_same_provider")
        }
    };
    attempts.push(FailoverAttempt {
        provider_id,
        provider_name: provider_name.to_string(),
        base_url: base_url.to_string(),
        outcome: outcome.to_string(),
        status: Some(StatusCode::BAD_GATEWAY.as_u16()),
        provider_index: Some(provider_index),
        retry_index: Some(retry_index),
        session_reuse,
        error_category: Some(ErrorCategory::SystemError.as_str()),
        error_code: Some(CODEX_REASONING_GUARD_ERROR_CODE),
        decision: Some(decision),
        reason: Some(format!(
            "codex reasoning guard matched reasoning_tokens={} {} {} via {} ({}) hit={} phase={} action={}",
            matched.reasoning_tokens,
            compare_mode_symbol(matched.compare_mode),
            matched.matched_rule_value,
            matched.pointer,
            matched.rule_source,
            budget.hit_number,
            budget.phase,
            budget.action_taken
        )),
        selection_method: dc::selection_method(provider_index, retry_index, session_reuse),
        reason_code: Some(CODEX_REASONING_GUARD_REASON_CODE),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_duration_ms),
        circuit_state_before: Some(circuit_state_before),
        circuit_state_after: Some(circuit_state_before),
        circuit_failure_count: Some(circuit_failure_count),
        circuit_failure_threshold: Some(circuit_failure_threshold),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget_decision_for_test(
        current_hits: u32,
        immediate_budget: u32,
        delayed_budget: u32,
        delayed_retry_ms: u32,
        exhausted_action: CodexReasoningGuardExhaustedAction,
    ) -> CodexReasoningGuardBudgetDecision {
        budget_decision(
            current_hits,
            immediate_budget,
            delayed_budget,
            delayed_retry_ms,
            exhausted_action,
            CodexReasoningGuardRetryPolicy::Single,
            5,
            1_000,
            10,
        )
    }

    #[test]
    fn detect_from_json_matches_equals_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516, 1024],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.reasoning_tokens, 516);
        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(matched.compare_mode, CodexReasoningGuardCompareMode::Equals);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
    }

    #[test]
    fn budget_decision_uses_immediate_then_delayed_budget() {
        for current_hits in 0..5 {
            let decision = budget_decision_for_test(
                current_hits,
                5,
                5,
                1_000,
                CodexReasoningGuardExhaustedAction::ReturnError,
            );
            assert_eq!(
                decision.action,
                CodexReasoningGuardBudgetAction::RetrySameProvider
            );
            assert_eq!(decision.phase, "immediate");
            assert_eq!(decision.delay_ms, 0);
            assert_eq!(decision.remaining_budget, 9 - current_hits);
        }

        for current_hits in 5..10 {
            let decision = budget_decision_for_test(
                current_hits,
                5,
                5,
                1_000,
                CodexReasoningGuardExhaustedAction::ReturnError,
            );
            assert_eq!(
                decision.action,
                CodexReasoningGuardBudgetAction::RetrySameProvider
            );
            assert_eq!(decision.phase, "delayed");
            assert_eq!(decision.delay_ms, 1_000);
            assert_eq!(decision.remaining_budget, 9 - current_hits);
        }
    }

    #[test]
    fn budget_decision_exhausts_to_configured_terminal_action() {
        let return_error = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            return_error.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );
        assert_eq!(return_error.phase, "exhausted");
        assert_eq!(return_error.remaining_budget, 0);

        let switch_provider = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::SwitchProvider,
        );
        assert_eq!(
            switch_provider.action,
            CodexReasoningGuardBudgetAction::SwitchProvider
        );
        assert_eq!(switch_provider.exhausted_action, "switch_provider");

        let switch_model = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::SwitchModel,
        );
        assert_eq!(
            switch_model.action,
            CodexReasoningGuardBudgetAction::SwitchModel
        );
        assert_eq!(switch_model.exhausted_action, "switch_model");
    }

    #[test]
    fn retry_wave_escalates_concurrent_policy_and_caps_at_max() {
        let first = retry_wave(0, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(first.concurrency, 1);
        assert!(!first.exhausted);

        let second = retry_wave(1, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(second.concurrency, 2);

        let capped = retry_wave(8, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(capped.concurrency, 5);
        assert!(!capped.exhausted);

        let exhausted = retry_wave(10, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(exhausted.concurrency, 5);
        assert!(exhausted.exhausted);
    }

    #[test]
    fn retry_wave_single_policy_never_escalates_concurrency() {
        let wave = retry_wave(4, CodexReasoningGuardRetryPolicy::Single, 5, 1_000, 10);
        assert_eq!(wave.concurrency, 1);
        assert_eq!(wave.hit_number, 5);
    }

    #[test]
    fn budget_decision_exhausts_when_concurrent_max_attempts_is_reached() {
        let decision = budget_decision(
            3,
            10,
            10,
            1_000,
            CodexReasoningGuardExhaustedAction::SwitchProvider,
            CodexReasoningGuardRetryPolicy::Concurrent,
            5,
            1_000,
            3,
        );

        assert_eq!(
            decision.action,
            CodexReasoningGuardBudgetAction::SwitchProvider
        );
        assert_eq!(decision.phase, "exhausted");
        assert_eq!(decision.retry_wave.expect("retry wave").hit_number, 4);
        assert!(decision.retry_wave.expect("retry wave").exhausted);
    }

    #[test]
    fn select_next_model_fallback_skips_blank_and_current_model() {
        let fallbacks = vec![
            " ".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.4".to_string(),
        ];
        assert_eq!(
            select_next_model_fallback(Some("gpt-5.5"), &fallbacks),
            Some("gpt-5.4")
        );
        assert_eq!(
            select_next_model_fallback(Some("gpt-5.3"), &fallbacks),
            Some("gpt-5.5")
        );
    }

    #[test]
    fn budget_decision_supports_zero_budget_edges() {
        let delayed_first = budget_decision_for_test(
            0,
            0,
            1,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            delayed_first.action,
            CodexReasoningGuardBudgetAction::RetrySameProvider
        );
        assert_eq!(delayed_first.phase, "delayed");
        assert_eq!(delayed_first.delay_ms, 500);

        let exhausted_first = budget_decision_for_test(
            0,
            0,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            exhausted_first.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );

        let immediate_only = budget_decision_for_test(
            1,
            2,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            immediate_only.action,
            CodexReasoningGuardBudgetAction::RetrySameProvider
        );
        assert_eq!(immediate_only.phase, "immediate");
        assert_eq!(immediate_only.remaining_budget, 0);

        let exhausted_after_immediate = budget_decision_for_test(
            2,
            2,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            exhausted_after_immediate.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );
        assert_eq!(exhausted_after_immediate.phase, "exhausted");
    }

    #[test]
    fn detect_from_json_does_not_match_equals_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[],
        );

        assert!(matched.is_none());
    }

    #[test]
    fn detect_from_json_matches_less_than_or_equal_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[516],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.reasoning_tokens, 300);
        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(
            matched.compare_mode,
            CodexReasoningGuardCompareMode::LessThanOrEqual
        );
    }

    #[test]
    fn detect_from_json_uses_smallest_matching_less_than_or_equal_threshold() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[1024, 516, 2048],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.matched_rule_value, 516);
    }

    #[test]
    fn detect_from_json_does_not_match_less_than_or_equal_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 800 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[516],
            &[],
        );

        assert!(matched.is_none());
    }

    #[test]
    fn detect_from_json_prefers_exact_model_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 600 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[CodexReasoningGuardModelRule {
                requested_model: "gpt-5-codex".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                reasoning_equals: vec![700],
            }],
        )
        .expect("should match model rule");

        assert_eq!(matched.matched_rule_value, 700);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE
        );
        assert_eq!(matched.rule_model.as_deref(), Some("gpt-5-codex"));
    }

    #[test]
    fn detect_from_json_falls_back_to_global_rule_when_model_rule_missing() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-mini-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[CodexReasoningGuardModelRule {
                requested_model: "gpt-5-codex".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                reasoning_equals: vec![700],
            }],
        )
        .expect("should fall back to global rule");

        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
        assert!(matched.rule_model.is_none());
    }
}
