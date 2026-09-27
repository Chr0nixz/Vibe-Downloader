/** Protocol helpers for TaskDetails diagnostics presentation. */

import type { TFunction } from "i18next";
import { resumeVerdict } from "@/components/tasks/row-recovery";
import type { TranslationKey } from "@/i18n";
import { formatDateTime } from "@/lib/format-date";
import type { Task } from "@/types/task";

export function isTorrentProtocol(protocol: string): boolean {
  return protocol === "bt" || protocol === "magnet";
}

export function isHlsProtocol(protocol: string): boolean {
  return protocol === "hls";
}

export function isDashProtocol(protocol: string): boolean {
  return protocol === "dash";
}

export function isFtpSftpProtocol(protocol: string): boolean {
  return protocol === "ftp" || protocol === "ftps" || protocol === "sftp";
}

export function isHttpLikeProtocol(protocol: string): boolean {
  return protocol === "http" || protocol === "https" || protocol.startsWith("webdav");
}

/** Show If-Range / ETag only for real HTTP-ish request methods. */
export function showsHttpRequestFields(method: string): boolean {
  const normalized = method.trim().toUpperCase();
  return (
    normalized === "GET" ||
    normalized === "HEAD" ||
    normalized === "POST" ||
    normalized === "PUT" ||
    normalized === "PATCH" ||
    normalized === "DELETE" ||
    normalized === "OPTIONS" ||
    normalized === "PROPFIND" ||
    normalized.startsWith("HTTP")
  );
}

export function diagnosticsSegmentsEmptyKey(protocol: string): TranslationKey {
  if (isHlsProtocol(protocol)) return "taskDetails.noHlsSegments";
  if (isDashProtocol(protocol)) return "taskDetails.noDashSegments";
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noChunks";
  return "taskDetails.noWorkUnits";
}

export function diagnosticsConnectionsEmptyKey(protocol: string): TranslationKey {
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noConnections";
  return "taskDetails.noWorkUnits";
}

export function diagnosticsRequestsEmptyKey(protocol: string): TranslationKey {
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noRequests";
  return "taskDetails.noRequestsGeneric";
}

export function defaultDiagSubTab(protocol: string): "segments" | "requests" {
  return isTorrentProtocol(protocol) ? "requests" : "segments";
}

/** TLS mode for the TaskDetails protocol row; each maps to a `ftpTls.*` key. */
export type FtpTlsMode = "plain" | "explicit" | "implicit";

/**
 * Align with FTP engine: FTPS on port 21 is explicit TLS; other FTPS ports
 * (default 990) are implicit TLS. Plain FTP has no TLS mode.
 */
export function ftpTlsModeLabel(protocol: string, url: string): FtpTlsMode | null {
  if (protocol === "ftp") return "plain";
  if (protocol !== "ftps") return null;
  try {
    const parsed = new URL(url);
    const port = parsed.port ? Number(parsed.port) : 990;
    return port === 21 ? "explicit" : "implicit";
  } catch {
    return "implicit";
  }
}

export function parseUrlHostPort(url: string, defaultPort: number): { host: string; port: number } | null {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname;
    if (!host) return null;
    const port = parsed.port ? Number(parsed.port) : defaultPort;
    return { host, port: Number.isFinite(port) ? port : defaultPort };
  } catch {
    return null;
  }
}

/** Health summaries that only restate the status badge next to the file name. */
const RESTATING_SUMMARIES = new Set([
  "taskDiagnostics.idle",
  "taskDiagnostics.downloading",
  "taskDiagnostics.completed",
  "taskDiagnostics.queued",
  "taskDiagnostics.waitingNetwork",
]);

/**
 * The details panel's one-line verdict: what the engine knows that the badge
 * and the progress numbers do not ("Server limit detected"). Null when there is
 * nothing to add, so the panel never prints a sentence for its own sake.
 */
export function detailDiagnosis(
  task: Pick<Task, "status" | "healthSummary" | "retryAfterAt">,
  t: TFunction,
): string | null {
  // The recovery block right below already states the problem and its cause.
  if (task.status === "failed" || task.status === "needs_attention") return null;
  if (task.status === "queued" && task.retryAfterAt) {
    return t("task.retryAfter", { time: formatDateTime(task.retryAfterAt, "time") });
  }
  const summary = task.healthSummary;
  // Only stable keys: raw engine text stays in the logs tab (UX-11), and an
  // English sentence here would ignore the chosen language.
  if (!summary?.startsWith("taskDiagnostics.") || RESTATING_SUMMARIES.has(summary)) return null;
  return t(summary as TranslationKey);
}

export interface CapabilityChip {
  labelKey: TranslationKey;
  /** Warning marks the one capability gap that can cost the user bytes. */
  tone: "neutral" | "warning";
}

/**
 * The server capabilities that explain a download's behaviour: whether it can
 * split into ranges and whether a pause or crash keeps the bytes. Only for
 * single-file transfer protocols, where both are server-decided; stream and
 * swarm protocols resume per segment or piece, so the same words would
 * mislead. Shown only while the transfer can still run: the flags come from
 * the first probe and are not cleared when a server later refuses a resume, so
 * on a stopped task "Resumable" could contradict the recovery block below.
 */
export function capabilityChips(
  task: Pick<Task, "status" | "protocol" | "supportsParallel" | "supportsResume"> &
    Partial<Pick<Task, "errorMessage" | "recoveryActions" | "errorCode">>,
): CapabilityChip[] {
  if (!isHttpLikeProtocol(task.protocol) && !isFtpSftpProtocol(task.protocol)) return [];
  if (!showsTransferRates(task.status)) return [];
  return [
    {
      labelKey: task.supportsParallel
        ? "taskDetails.capability.rangeSupported"
        : "taskDetails.capability.singleConnection",
      tone: "neutral",
    },
    (
      task.status !== "paused" && task.status !== "waiting_network"
        ? task.supportsResume
        : resumeVerdict(task) === "available"
    )
      ? { labelKey: "taskDetails.capability.resumable", tone: "neutral" }
      : { labelKey: "taskDetails.capability.notResumable", tone: "warning" },
  ];
}

/** Speed and ETA describe a transfer in progress; for a stopped task they are
 * two rows of dashes. */
export function showsTransferRates(status: Task["status"]): boolean {
  return status !== "completed" && status !== "failed" && status !== "needs_attention";
}
