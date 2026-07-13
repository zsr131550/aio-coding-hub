import { describe, expect, it } from "vitest";
import {
  chooseModelRouteAwareSpecialSettingsJson,
  countCodexReasoningGuardSpecialSettings,
  formatCodexReasoningEffortSource,
  hasModelRouteMappingSpecialSetting,
  hasClaudeModelMappingSpecialSetting,
  resolveCodexReasoningEffort,
  resolveCodexReasoningGuardSummary,
  resolveClaudeModelMappingFromSpecialSettings,
  resolveModelRouteMappingFromSpecialSettings,
} from "../requestLogSpecialSettings";

describe("services/gateway/requestLogSpecialSettings", () => {
  it("resolves Claude model mapping with final provider preference", () => {
    const settings = JSON.stringify([
      { type: "noop" },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-4.1 ",
        mappingKind: " sonnet ",
        providerId: 1,
        providerName: " Provider A ",
        applied: true,
      },
      {
        type: "claude_model_mapping",
        requestedModel: " claude-sonnet ",
        effectiveModel: " gpt-5.4 ",
        mappingKind: " sonnet ",
        providerId: 2,
        providerName: " Provider B ",
        applied: true,
      },
    ]);

    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 2)).toEqual({
      requestedModel: "claude-sonnet",
      effectiveModel: "gpt-5.4",
      mappingKind: "sonnet",
      providerId: 2,
      providerName: "Provider B",
      applied: true,
    });
    expect(resolveClaudeModelMappingFromSpecialSettings(settings, 99)?.providerId).toBe(2);
    expect(hasClaudeModelMappingSpecialSetting(settings)).toBe(true);
  });

  it("ignores invalid, unapplied, and identity mappings", () => {
    expect(resolveClaudeModelMappingFromSpecialSettings(null)).toBeNull();
    expect(resolveClaudeModelMappingFromSpecialSettings("bad-json")).toBeNull();
    expect(
      resolveClaudeModelMappingFromSpecialSettings(
        JSON.stringify([
          {
            type: "claude_model_mapping",
            requestedModel: "same",
            effectiveModel: "same",
            mappingKind: "sonnet",
            providerId: 1,
            providerName: "Provider A",
            applied: true,
          },
          {
            type: "claude_model_mapping",
            requestedModel: "claude-sonnet",
            effectiveModel: "gpt-5.4",
            mappingKind: "sonnet",
            providerId: 2,
            providerName: "Provider B",
            applied: false,
          },
        ])
      )
    ).toBeNull();
    expect(hasClaudeModelMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("counts Codex reasoning guard special settings", () => {
    expect(
      countCodexReasoningGuardSpecialSettings(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            compareModeSymbol: "==",
            matchedRuleValue: 516,
            reasoningTokens: 516,
          },
          { type: "noop" },
          {
            type: "codex_reasoning_guard",
            compareMode: "less_than_or_equal",
            compareModeSymbol: "<=",
            matchedRuleValue: 516,
            reasoningTokens: 300,
          },
        ])
      )
    ).toBe(2);
    expect(
      countCodexReasoningGuardSpecialSettings(JSON.stringify({ type: "codex_reasoning_guard" }))
    ).toBe(1);
    expect(countCodexReasoningGuardSpecialSettings("bad-json")).toBe(0);
  });

  it("resolves Codex reasoning guard summary with latest rule label", () => {
    expect(
      resolveCodexReasoningGuardSummary(
        JSON.stringify([
          {
            type: "codex_reasoning_guard",
            compareMode: "equals",
            matchedRuleValue: 516,
            reasoningTokens: 516,
            guardRetryPhase: "immediate",
            actionTaken: "retry_same_provider_no_circuit",
            guardBudgetRemaining: 9,
            guardBudgetTotal: 10,
          },
          {
            type: "codex_reasoning_guard",
            compareMode: "less_than_or_equal",
            compareModeSymbol: "<=",
            matchedRuleValue: 516,
            reasoningTokens: 300,
            guardRetryPhase: "delayed",
            actionTaken: "retry_same_provider_delayed_no_circuit",
            guardExhaustedAction: "return_error",
            backoffMs: 1000,
            guardBudgetRemaining: 4,
            guardBudgetTotal: 10,
          },
        ])
      )
    ).toEqual({
      count: 2,
      latestRuleLabel: "<= 516",
      latestReasoningTokens: 300,
      latestPhase: "delayed",
      latestActionTaken: "retry_same_provider_delayed_no_circuit",
      latestExhaustedAction: "return_error",
      latestDelayMs: 1000,
      latestBudgetRemaining: 4,
      latestBudgetTotal: 10,
    });
  });

  it("resolves explicit Codex reasoning effort from special settings", () => {
    const settings = JSON.stringify([
      {
        type: "codex_reasoning_effort",
        source: "request",
        effort: " HIGH ",
      },
    ]);

    expect(resolveCodexReasoningEffort("gpt-5.5", settings)).toEqual({
      effort: "high",
      source: "request",
    });
  });

  it("uses conservative Codex effort defaults and unknown fallback", () => {
    expect(resolveCodexReasoningEffort(" gpt-5.5 ", null)).toEqual({
      effort: "medium",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.4-mini", "bad-json")).toEqual({
      effort: "low",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.5-pro", null)).toEqual({
      effort: "high",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-5.4-pro", null)).toEqual({
      effort: "medium",
      source: "default",
    });
    expect(resolveCodexReasoningEffort("gpt-future", null)).toEqual({
      effort: "unknown",
      source: "unknown",
    });
  });

  it("does not use defaults when an explicit Codex reasoning effort is invalid", () => {
    const settings = JSON.stringify([
      {
        type: "codex_reasoning_effort",
        source: "request",
        rawEffort: "turbo",
      },
    ]);

    expect(resolveCodexReasoningEffort("gpt-5.5", settings)).toEqual({
      effort: "unknown",
      source: "unknown",
    });
  });

  it("formats Codex reasoning effort source labels", () => {
    expect(formatCodexReasoningEffortSource("request")).toBe("请求显式");
    expect(formatCodexReasoningEffortSource("default")).toBe("默认推断");
    expect(formatCodexReasoningEffortSource("unknown")).toBe("未知");
  });

  it("resolves model route mapping with final provider preference and effort-only mismatches", () => {
    const settings = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.5",
        actualReasoningEffort: "medium",
        actualReasoningEffortSource: "model_default",
        modelMismatch: false,
        effortMismatch: true,
        mismatch: true,
        providerId: 1,
        providerName: "Provider A",
      },
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        actualReasoningEffortSource: "model_default",
        modelMismatch: true,
        effortMismatch: true,
        mismatch: true,
        providerId: 2,
        providerName: "Provider B",
      },
    ]);

    expect(resolveModelRouteMappingFromSpecialSettings(settings, 1)).toMatchObject({
      requestedModel: "gpt-5.5",
      actualModel: "gpt-5.5",
      requestedReasoningEffort: "high",
      actualReasoningEffort: "medium",
      modelMismatch: false,
      effortMismatch: true,
      providerId: 1,
    });
    expect(resolveModelRouteMappingFromSpecialSettings(settings, 99)).toBeNull();
    expect(hasModelRouteMappingSpecialSetting(settings)).toBe(true);
  });

  it("does not fall back to another provider mapping when provider-scoped routes do not match", () => {
    const settings = JSON.stringify([
      {
        type: "model_route_mapping",
        cliKey: "codex",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        requestedReasoningEffortSource: "request",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        actualReasoningEffortSource: "model_default",
        modelMismatch: true,
        effortMismatch: true,
        mismatch: true,
        providerId: 2,
        providerName: "Provider B",
      },
    ]);

    expect(resolveModelRouteMappingFromSpecialSettings(settings, 1)).toBeNull();
    expect(resolveModelRouteMappingFromSpecialSettings(settings, null)).toMatchObject({
      providerId: 2,
      actualModel: "gpt-5.4-mini",
    });
  });

  it("ignores invalid and identity model route mappings", () => {
    expect(resolveModelRouteMappingFromSpecialSettings(null)).toBeNull();
    expect(resolveModelRouteMappingFromSpecialSettings("bad-json")).toBeNull();
    expect(
      resolveModelRouteMappingFromSpecialSettings(
        JSON.stringify([
          {
            type: "model_route_mapping",
            requestedModel: "GPT-5.5",
            actualModel: "gpt-5.5",
            requestedReasoningEffort: "medium",
            actualReasoningEffort: "medium",
            modelMismatch: false,
            effortMismatch: false,
            mismatch: false,
          },
          {
            type: "model_route_mapping",
            requestedModel: "",
            actualModel: "gpt-5.4-mini",
            mismatch: true,
          },
        ])
      )
    ).toBeNull();
    expect(hasModelRouteMappingSpecialSetting("bad-json")).toBe(false);
  });

  it("chooses model-route-aware special settings ahead of stale start settings", () => {
    const startSettings = JSON.stringify([
      { type: "codex_reasoning_effort", source: "request", effort: "high" },
    ]);
    const terminalSettings = JSON.stringify([
      {
        type: "model_route_mapping",
        requestedModel: "gpt-5.5",
        requestedReasoningEffort: "high",
        actualModel: "gpt-5.4-mini",
        actualReasoningEffort: "low",
        mismatch: true,
      },
    ]);

    expect(chooseModelRouteAwareSpecialSettingsJson(terminalSettings, startSettings)).toBe(
      terminalSettings
    );
    expect(chooseModelRouteAwareSpecialSettingsJson(startSettings, terminalSettings)).toBe(
      terminalSettings
    );
    expect(chooseModelRouteAwareSpecialSettingsJson("bad-json", startSettings)).toBe(startSettings);
  });
});
