import { isPersistedRequestLogInProgress, requestLogCreatedAtMs } from "./requestLogState";
import type { RequestLogSummary } from "./requestLogs";
import { mergeTraceWithRequestLog } from "./traceRequestLogMerge";
import type { TraceSession } from "./traceStore";

export const REALTIME_TRACE_EXIT_START_MS = 600;
export const REALTIME_TRACE_EXIT_ANIM_MS = 400;
export const REALTIME_TRACE_EXIT_TOTAL_MS =
  REALTIME_TRACE_EXIT_START_MS + REALTIME_TRACE_EXIT_ANIM_MS + 100;

export type ProjectedRealtimeCard = {
  trace: TraceSession;
};

export type ProjectedRequestLogRow = {
  log: RequestLogSummary;
  liveTrace: TraceSession | null;
};

export type RequestActivityProjection = {
  realtimeCards: ProjectedRealtimeCard[];
  requestRows: ProjectedRequestLogRow[];
  visibleRealtimeTraceIds: Set<string>;
  hasPending: boolean;
  hasLiveRealtimeCards: boolean;
  summaryCount: number;
};

export type BuildRequestActivityProjectionInput = {
  requestLogs: RequestLogSummary[];
  traces: TraceSession[];
  nowMs: number;
  realtimeCardLimit: number;
  realtimeCandidateLimit: number;
};

function normalizeTraceId(traceId: string | null | undefined) {
  return traceId?.trim() || null;
}

export function sortRequestLogsForActivity(a: RequestLogSummary, b: RequestLogSummary) {
  const aInProgress = isPersistedRequestLogInProgress(a);
  const bInProgress = isPersistedRequestLogInProgress(b);
  if (aInProgress !== bInProgress) return aInProgress ? -1 : 1;

  const aTsMs = requestLogCreatedAtMs(a);
  const bTsMs = requestLogCreatedAtMs(b);
  if (aTsMs !== bTsMs) return bTsMs - aTsMs;
  return b.id - a.id;
}

export function shouldKeepProjectedRealtimeTraceVisible(trace: TraceSession, nowMs: number) {
  if (!trace.summary) return true;
  return Math.max(0, nowMs - trace.last_seen_ms) < REALTIME_TRACE_EXIT_TOTAL_MS;
}

export function buildRequestActivityProjection({
  requestLogs,
  traces,
  nowMs,
  realtimeCardLimit,
  realtimeCandidateLimit,
}: BuildRequestActivityProjectionInput): RequestActivityProjection {
  const requestRowsSorted = requestLogs.slice().sort(sortRequestLogsForActivity);
  const logsByTraceId = new Map<string, RequestLogSummary>();
  for (const log of requestRowsSorted) {
    const traceId = normalizeTraceId(log.trace_id);
    if (!traceId || logsByTraceId.has(traceId)) continue;
    logsByTraceId.set(traceId, log);
  }

  const mergedTraceMap = new Map<string, TraceSession>();
  for (const trace of traces) {
    const traceId = normalizeTraceId(trace.trace_id);
    if (!traceId || mergedTraceMap.has(traceId)) continue;
    mergedTraceMap.set(traceId, mergeTraceWithRequestLog(trace, logsByTraceId.get(traceId)));
  }

  const realtimeCandidates = Array.from(mergedTraceMap.values())
    .filter((trace) => shouldKeepProjectedRealtimeTraceVisible(trace, nowMs))
    .sort((a, b) => b.first_seen_ms - a.first_seen_ms)
    .slice(0, realtimeCandidateLimit);

  const realtimeCards = realtimeCandidates.slice(0, realtimeCardLimit).map((trace) => ({ trace }));
  const visibleRealtimeTraceIds = new Set(
    realtimeCards
      .map((card) => normalizeTraceId(card.trace.trace_id))
      .filter((traceId): traceId is string => traceId != null)
  );

  const requestRows = requestRowsSorted
    .filter((log) => {
      const traceId = normalizeTraceId(log.trace_id);
      return !traceId || !visibleRealtimeTraceIds.has(traceId);
    })
    .map((log) => {
      const traceId = normalizeTraceId(log.trace_id);
      return {
        log,
        liveTrace: traceId ? (mergedTraceMap.get(traceId) ?? null) : null,
      };
    });

  return {
    realtimeCards,
    requestRows,
    visibleRealtimeTraceIds,
    hasPending: requestRowsSorted.some((log) => isPersistedRequestLogInProgress(log)),
    hasLiveRealtimeCards: realtimeCards.length > 0,
    summaryCount: requestRowsSorted.length,
  };
}
