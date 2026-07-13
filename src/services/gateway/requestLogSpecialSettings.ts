import { normalizeClaudeModelMapping, type ClaudeModelMapping } from "./claudeModelMapping";

export type ParsedRequestLogSpecialSetting = {
  type?: string;
  reason?: string;
} & Record<string, unknown>;

export type CodexReasoningGuardSummary = {
  count: number;
  latestRuleLabel: string | null;
  latestReasoningTokens: number | null;
  latestPhase: string | null;
  latestActionTaken: string | null;
  latestExhaustedAction: string | null;
  latestDelayMs: number | null;
  latestBudgetRemaining: number | null;
  latestBudgetTotal: number | null;
};

export type CodexReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "unknown";

export type CodexReasoningEffortSource = "request" | "default" | "unknown";
export type ModelRouteReasoningEffortSource =
  | CodexReasoningEffortSource
  | "model_default"
  | "response";

export type CodexReasoningEffortResolution = {
  effort: CodexReasoningEffort;
  source: CodexReasoningEffortSource;
};

export type ModelRouteMapping = {
  cliKey: string;
  requestedModel: string;
  requestedReasoningEffort: CodexReasoningEffort;
  requestedReasoningEffortSource: ModelRouteReasoningEffortSource;
  actualModel: string;
  actualReasoningEffort: CodexReasoningEffort;
  actualReasoningEffortSource: ModelRouteReasoningEffortSource;
  modelMismatch: boolean;
  effortMismatch: boolean;
  mismatch: boolean;
  providerId: number | null;
  providerName: string | null;
};

const CODEX_REASONING_EFFORTS = new Set<CodexReasoningEffort>([
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
]);

const KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS: Readonly<Record<string, CodexReasoningEffort>> =
  {
    "gpt-5.5": "medium",
    "gpt-5.5-pro": "high",
    "gpt-5.4": "none",
    "gpt-5.4-mini": "low",
    "gpt-5.4-nano": "none",
    "gpt-5.4-pro": "medium",
  };

const CODEX_REASONING_EFFORT_FIELD_NAMES = new Set(["effort", "rawEffort"]);

export function parseRequestLogSpecialSettings(
  specialSettingsJson: string | null | undefined
): ParsedRequestLogSpecialSetting[] {
  if (!specialSettingsJson) return [];

  try {
    const parsed = JSON.parse(specialSettingsJson) as unknown;
    if (Array.isArray(parsed)) {
      return parsed.filter(isParsedRequestLogSpecialSetting);
    }
    return isParsedRequestLogSpecialSetting(parsed) ? [parsed] : [];
  } catch {
    return [];
  }
}

function isParsedRequestLogSpecialSetting(value: unknown): value is ParsedRequestLogSpecialSetting {
  return typeof value === "object" && value !== null;
}

function parsedSettingString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function parsedSettingNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : Number.NaN;
}

function parsedSettingBoolean(value: unknown): boolean {
  return typeof value === "boolean" ? value : false;
}

function parsedSettingOptionalBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function normalizeCodexReasoningEffort(
  value: unknown
): Exclude<CodexReasoningEffort, "unknown"> | null {
  const effort = parsedSettingString(value).trim().toLowerCase();
  return CODEX_REASONING_EFFORTS.has(effort as CodexReasoningEffort)
    ? (effort as Exclude<CodexReasoningEffort, "unknown">)
    : null;
}

function normalizeModelRouteReasoningEffort(value: unknown): CodexReasoningEffort {
  return normalizeCodexReasoningEffort(value) ?? "unknown";
}

function normalizeModelRouteReasoningEffortSource(value: unknown): ModelRouteReasoningEffortSource {
  const source = parsedSettingString(value).trim().toLowerCase();
  if (source === "request") return "request";
  if (source === "default") return "default";
  if (source === "model_default") return "model_default";
  if (source === "response") return "response";
  return "unknown";
}

function normalizeRequestedModel(value: string | null | undefined): string | null {
  const model = value?.trim().toLowerCase();
  return model ? model : null;
}

export function resolveCodexReasoningEffort(
  requestedModel: string | null | undefined,
  specialSettingsJson: string | null | undefined
): CodexReasoningEffortResolution {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const explicitSetting = settings
    .slice()
    .reverse()
    .find((setting) => setting.type === "codex_reasoning_effort");
  const explicitEffort = explicitSetting
    ? normalizeCodexReasoningEffort(explicitSetting.effort)
    : null;

  if (explicitEffort) {
    return { effort: explicitEffort, source: "request" };
  }

  if (explicitSetting && hasCodexReasoningEffortField(explicitSetting)) {
    return { effort: "unknown", source: "unknown" };
  }

  const model = normalizeRequestedModel(requestedModel);
  if (model && KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS[model]) {
    return {
      effort: KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS[model],
      source: "default",
    };
  }

  return { effort: "unknown", source: "unknown" };
}

function hasCodexReasoningEffortField(setting: ParsedRequestLogSpecialSetting): boolean {
  return Object.keys(setting).some((key) => CODEX_REASONING_EFFORT_FIELD_NAMES.has(key));
}

export function formatCodexReasoningEffortSource(source: CodexReasoningEffortSource): string {
  if (source === "request") return "请求显式";
  if (source === "default") return "默认推断";
  return "未知";
}

export function formatModelRouteReasoningEffortSource(
  source: ModelRouteReasoningEffortSource
): string {
  if (source === "request") return "请求显式";
  if (source === "default") return "默认推断";
  if (source === "model_default") return "模型默认推断";
  if (source === "response") return "返回显式";
  return "未知";
}

function normalizeRouteText(value: unknown): string | null {
  const text = parsedSettingString(value).trim();
  return text ? text : null;
}

function normalizeRouteNumber(value: unknown): number | null {
  const number = parsedSettingNumber(value);
  return Number.isFinite(number) ? number : null;
}

function sameRouteText(left: string, right: string): boolean {
  return left.trim().toLowerCase() === right.trim().toLowerCase();
}

function normalizeModelRouteMappingSetting(
  setting: ParsedRequestLogSpecialSetting
): ModelRouteMapping | null {
  if (setting.type !== "model_route_mapping") return null;

  const requestedModel = normalizeRouteText(setting.requestedModel);
  const actualModel = normalizeRouteText(setting.actualModel);
  if (!requestedModel || !actualModel) return null;

  const requestedReasoningEffort = normalizeModelRouteReasoningEffort(
    setting.requestedReasoningEffort
  );
  const actualReasoningEffort = normalizeModelRouteReasoningEffort(setting.actualReasoningEffort);
  const modelMismatch =
    parsedSettingOptionalBoolean(setting.modelMismatch) ??
    !sameRouteText(requestedModel, actualModel);
  const inferredEffortMismatch =
    requestedReasoningEffort !== "unknown" &&
    actualReasoningEffort !== "unknown" &&
    requestedReasoningEffort !== actualReasoningEffort;
  const effortMismatch =
    parsedSettingOptionalBoolean(setting.effortMismatch) ?? inferredEffortMismatch;
  const mismatch =
    parsedSettingOptionalBoolean(setting.mismatch) ?? (modelMismatch || effortMismatch);

  if (!mismatch && !modelMismatch && !effortMismatch) return null;

  return {
    cliKey: normalizeRouteText(setting.cliKey) ?? "",
    requestedModel,
    requestedReasoningEffort,
    requestedReasoningEffortSource: normalizeModelRouteReasoningEffortSource(
      setting.requestedReasoningEffortSource
    ),
    actualModel,
    actualReasoningEffort,
    actualReasoningEffortSource: normalizeModelRouteReasoningEffortSource(
      setting.actualReasoningEffortSource
    ),
    modelMismatch,
    effortMismatch,
    mismatch: true,
    providerId: normalizeRouteNumber(setting.providerId),
    providerName: normalizeRouteText(setting.providerName),
  };
}

export function resolveModelRouteMappingFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): ModelRouteMapping | null {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const mappings = settings
    .map(normalizeModelRouteMappingSetting)
    .filter((mapping): mapping is ModelRouteMapping => mapping !== null);

  if (mappings.length === 0) return null;

  if (finalProviderId != null) {
    const finalProviderMapping = mappings
      .slice()
      .reverse()
      .find((mapping) => mapping.providerId === finalProviderId);
    if (finalProviderMapping) return finalProviderMapping;

    if (mappings.some((mapping) => mapping.providerId != null)) {
      return null;
    }
  }

  return mappings[mappings.length - 1] ?? null;
}

export function hasModelRouteMappingSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  return resolveModelRouteMappingFromSpecialSettings(specialSettingsJson) !== null;
}

function hasValidSpecialSettingsJson(value: string | null | undefined): boolean {
  return parseRequestLogSpecialSettings(value).length > 0;
}

export function chooseModelRouteAwareSpecialSettingsJson(
  preferredSettings: string | null | undefined,
  fallbackSettings: string | null | undefined
): string | null {
  const preferredHasRoute = hasModelRouteMappingSpecialSetting(preferredSettings);
  const fallbackHasRoute = hasModelRouteMappingSpecialSetting(fallbackSettings);
  if (preferredHasRoute) return preferredSettings ?? null;
  if (fallbackHasRoute) return fallbackSettings ?? null;

  if (hasValidSpecialSettingsJson(preferredSettings)) return preferredSettings ?? null;
  if (hasValidSpecialSettingsJson(fallbackSettings)) return fallbackSettings ?? null;

  return preferredSettings ?? fallbackSettings ?? null;
}

export function resolveClaudeModelMappingFromSpecialSettings(
  specialSettingsJson: string | null | undefined,
  finalProviderId?: number | null
): ClaudeModelMapping | null {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  const mappings = settings
    .map((setting) => {
      if (setting.type !== "claude_model_mapping") return null;
      return normalizeClaudeModelMapping({
        requestedModel: parsedSettingString(setting.requestedModel),
        effectiveModel: parsedSettingString(setting.effectiveModel),
        mappingKind: parsedSettingString(setting.mappingKind),
        providerId: parsedSettingNumber(setting.providerId),
        providerName: parsedSettingString(setting.providerName),
        applied: parsedSettingBoolean(setting.applied),
      });
    })
    .filter((mapping): mapping is ClaudeModelMapping => mapping !== null);

  if (mappings.length === 0) return null;

  if (finalProviderId != null) {
    const finalProviderMapping = mappings
      .slice()
      .reverse()
      .find((mapping) => mapping.providerId === finalProviderId);
    if (finalProviderMapping) return finalProviderMapping;
  }

  return mappings[mappings.length - 1] ?? null;
}

export function hasClaudeModelMappingSpecialSetting(
  specialSettingsJson: string | null | undefined
): boolean {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  for (const setting of settings) {
    if (setting.type !== "claude_model_mapping") continue;
    return true;
  }
  return false;
}

export function countCodexReasoningGuardSpecialSettings(
  specialSettingsJson: string | null | undefined
): number {
  return resolveCodexReasoningGuardSummary(specialSettingsJson).count;
}

function normalizeCodexReasoningGuardCompareSymbol(
  compareMode: unknown,
  compareModeSymbol: unknown
): string | null {
  const explicitSymbol = parsedSettingString(compareModeSymbol);
  if (explicitSymbol === "==" || explicitSymbol === "<=") {
    return explicitSymbol;
  }

  const mode = parsedSettingString(compareMode);
  if (mode === "equals") return "==";
  if (mode === "less_than_or_equal") return "<=";
  return null;
}

export function resolveCodexReasoningGuardSummary(
  specialSettingsJson: string | null | undefined
): CodexReasoningGuardSummary {
  const settings = parseRequestLogSpecialSettings(specialSettingsJson);
  let count = 0;
  let latestRuleLabel: string | null = null;
  let latestReasoningTokens: number | null = null;
  let latestPhase: string | null = null;
  let latestActionTaken: string | null = null;
  let latestExhaustedAction: string | null = null;
  let latestDelayMs: number | null = null;
  let latestBudgetRemaining: number | null = null;
  let latestBudgetTotal: number | null = null;

  for (const setting of settings) {
    if (setting.type === "codex_reasoning_guard") {
      count += 1;
      const compareSymbol = normalizeCodexReasoningGuardCompareSymbol(
        setting.compareMode,
        setting.compareModeSymbol
      );
      const matchedRuleValue = parsedSettingNumber(setting.matchedRuleValue);
      const reasoningTokens = parsedSettingNumber(setting.reasoningTokens);
      latestRuleLabel =
        compareSymbol && Number.isFinite(matchedRuleValue)
          ? `${compareSymbol} ${matchedRuleValue}`
          : null;
      latestReasoningTokens = Number.isFinite(reasoningTokens) ? reasoningTokens : null;
      latestPhase = parsedSettingString(setting.guardRetryPhase) || null;
      latestActionTaken =
        parsedSettingString(setting.actionTaken) || parsedSettingString(setting.action) || null;
      latestExhaustedAction = parsedSettingString(setting.guardExhaustedAction) || null;
      const delayMs = parsedSettingNumber(setting.backoffMs);
      const budgetRemaining = parsedSettingNumber(setting.guardBudgetRemaining);
      const budgetTotal = parsedSettingNumber(setting.guardBudgetTotal);
      latestDelayMs = Number.isFinite(delayMs) ? delayMs : null;
      latestBudgetRemaining = Number.isFinite(budgetRemaining) ? budgetRemaining : null;
      latestBudgetTotal = Number.isFinite(budgetTotal) ? budgetTotal : null;
    }
  }

  return {
    count,
    latestRuleLabel,
    latestReasoningTokens,
    latestPhase,
    latestActionTaken,
    latestExhaustedAction,
    latestDelayMs,
    latestBudgetRemaining,
    latestBudgetTotal,
  };
}
