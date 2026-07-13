//! Build compact audit settings for requested-vs-returned model routing.

use serde_json::{json, Value};

const KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS: &[(&str, &str)] = &[
    ("gpt-5.5", "medium"),
    ("gpt-5.5-pro", "high"),
    ("gpt-5.4", "none"),
    ("gpt-5.4-mini", "low"),
    ("gpt-5.4-nano", "none"),
    ("gpt-5.4-pro", "medium"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway) struct ModelRouteSettingInput<'a> {
    pub(in crate::gateway) cli_key: &'a str,
    pub(in crate::gateway) requested_model: Option<&'a str>,
    pub(in crate::gateway) actual_model: Option<&'a str>,
    pub(in crate::gateway) special_settings: &'a [Value],
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: &'a str,
}

pub(in crate::gateway) fn build_model_route_mapping_setting(
    input: ModelRouteSettingInput<'_>,
) -> Option<Value> {
    let requested_model = normalize_text(input.requested_model)?;
    let actual_model = normalize_text(input.actual_model)?;
    let requested_effort =
        resolve_requested_effort(input.cli_key, &requested_model, input.special_settings);
    let actual_effort = resolve_actual_effort(input.cli_key, &actual_model);
    let model_mismatch = !same_route_part(&requested_model, &actual_model);
    let effort_mismatch = match (&requested_effort.effort, &actual_effort.effort) {
        (Some(requested), Some(actual)) => !same_route_part(requested, actual),
        _ => false,
    };

    if !model_mismatch && !effort_mismatch {
        return None;
    }

    Some(json!({
        "type": "model_route_mapping",
        "cliKey": input.cli_key,
        "requestedModel": requested_model,
        "requestedReasoningEffort": requested_effort.effort,
        "requestedReasoningEffortSource": requested_effort.source,
        "actualModel": actual_model,
        "actualReasoningEffort": actual_effort.effort,
        "actualReasoningEffortSource": actual_effort.source,
        "modelMismatch": model_mismatch,
        "effortMismatch": effort_mismatch,
        "mismatch": true,
        "providerId": input.provider_id,
        "providerName": normalize_text(Some(input.provider_name)),
    }))
}

pub(in crate::gateway) fn build_model_route_mapping_setting_from_shared(
    cli_key: &str,
    requested_model: Option<&str>,
    actual_model: Option<&str>,
    special_settings: &std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    provider_id: i64,
    provider_name: &str,
) -> Option<Value> {
    let settings = special_settings.lock().ok()?.clone();
    build_model_route_mapping_setting(ModelRouteSettingInput {
        cli_key,
        requested_model,
        actual_model,
        special_settings: settings.as_slice(),
        provider_id,
        provider_name,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffortResolution {
    effort: Option<String>,
    source: &'static str,
}

fn resolve_requested_effort(
    cli_key: &str,
    requested_model: &str,
    special_settings: &[Value],
) -> EffortResolution {
    if cli_key != "codex" {
        return EffortResolution {
            effort: None,
            source: "unknown",
        };
    }

    let explicit = special_settings.iter().rev().find(|setting| {
        setting.get("type").and_then(Value::as_str) == Some("codex_reasoning_effort")
    });
    if let Some(setting) = explicit {
        return EffortResolution {
            effort: setting
                .get("effort")
                .and_then(Value::as_str)
                .and_then(normalize_effort),
            source: "request",
        };
    }

    EffortResolution {
        effort: default_codex_reasoning_effort(requested_model).map(str::to_string),
        source: "model_default",
    }
}

fn resolve_actual_effort(cli_key: &str, actual_model: &str) -> EffortResolution {
    if cli_key != "codex" {
        return EffortResolution {
            effort: None,
            source: "unknown",
        };
    }

    EffortResolution {
        effort: default_codex_reasoning_effort(actual_model).map(str::to_string),
        source: "model_default",
    }
}

fn default_codex_reasoning_effort(model: &str) -> Option<&'static str> {
    let normalized = model.trim().to_ascii_lowercase();
    KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS
        .iter()
        .find_map(|(known_model, effort)| (*known_model == normalized).then_some(*effort))
}

fn normalize_effort(value: &str) -> Option<String> {
    let effort = value.trim().to_ascii_lowercase();
    matches!(
        effort.as_str(),
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
    )
    .then_some(effort)
}

fn normalize_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(if value.chars().count() > 200 {
        value.chars().take(200).collect()
    } else {
        value.to_string()
    })
}

fn same_route_part(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_codex_model_mismatch_with_effort_sources() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some(" gpt-5.5 "),
            actual_model: Some("gpt-5.4-mini"),
            special_settings: &[json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            })],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("setting");

        assert_eq!(
            setting.get("type").and_then(Value::as_str),
            Some("model_route_mapping")
        );
        assert_eq!(
            setting.get("requestedModel").and_then(Value::as_str),
            Some("gpt-5.5")
        );
        assert_eq!(
            setting
                .get("requestedReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            setting.get("actualModel").and_then(Value::as_str),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            setting.get("actualReasoningEffort").and_then(Value::as_str),
            Some("low")
        );
        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn skips_identity_route_even_when_case_differs() {
        assert!(build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("GPT-5.5"),
            actual_model: Some("gpt-5.5"),
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .is_none());
    }

    #[test]
    fn builds_same_model_effort_mismatch_when_both_efforts_are_known() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some("gpt-5.5"),
            special_settings: &[json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            })],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("effort mismatch");

        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting
                .get("requestedReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            setting.get("actualReasoningEffort").and_then(Value::as_str),
            Some("medium")
        );
        assert_eq!(
            setting
                .get("actualReasoningEffortSource")
                .and_then(Value::as_str),
            Some("model_default")
        );
    }

    #[test]
    fn does_not_treat_unknown_effort_as_effort_mismatch() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-future"),
            actual_model: Some("gpt-other"),
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("model mismatch");

        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(false)
        );
        assert!(setting
            .get("actualReasoningEffort")
            .is_some_and(Value::is_null));
    }

    #[test]
    fn truncates_route_text_on_utf8_char_boundaries() {
        let long_unicode_model = "模型".repeat(201);
        let long_provider_name = "供应商".repeat(201);
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some(&long_unicode_model),
            special_settings: &[],
            provider_id: 7,
            provider_name: &long_provider_name,
        })
        .expect("model mismatch");

        let actual = setting
            .get("actualModel")
            .and_then(Value::as_str)
            .expect("actual model");
        assert_eq!(actual.chars().count(), 200);
        assert!(actual.is_char_boundary(actual.len()));

        let provider_name = setting
            .get("providerName")
            .and_then(Value::as_str)
            .expect("provider name");
        assert_eq!(provider_name.chars().count(), 200);
        assert!(provider_name.is_char_boundary(provider_name.len()));
    }
}
