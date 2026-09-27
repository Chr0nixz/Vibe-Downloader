import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { Task } from "@/types/task";
import {
  capabilityChips,
  defaultDiagSubTab,
  detailDiagnosis,
  diagnosticsRequestsEmptyKey,
  diagnosticsSegmentsEmptyKey,
  isHlsProtocol,
  isTorrentProtocol,
  showsHttpRequestFields,
  showsTransferRates,
} from "./task-diagnostics";

const t = ((key: string) => key) as unknown as TFunction;

function diagnosisTask(overrides: Partial<Task>): Pick<Task, "status" | "healthSummary" | "retryAfterAt"> {
  return { status: "downloading", healthSummary: null, retryAfterAt: null, ...overrides };
}

describe("details overview diagnosis", () => {
  it("states what the engine knows beyond the badge", () => {
    expect(detailDiagnosis(diagnosisTask({ healthSummary: "taskDiagnostics.serverLimitDetected" }), t)).toBe(
      "taskDiagnostics.serverLimitDetected",
    );
    expect(detailDiagnosis(diagnosisTask({ status: "queued", retryAfterAt: "2026-01-01T10:00:00.000Z" }), t)).toBe(
      "task.retryAfter",
    );
  });

  it("says nothing rather than repeat the badge or leak engine text", () => {
    expect(detailDiagnosis(diagnosisTask({ healthSummary: "taskDiagnostics.downloading" }), t)).toBeNull();
    expect(
      detailDiagnosis(diagnosisTask({ status: "completed", healthSummary: "taskDiagnostics.completed" }), t),
    ).toBeNull();
    expect(detailDiagnosis(diagnosisTask({ healthSummary: "Fetching torrent metadata" }), t)).toBeNull();
    expect(detailDiagnosis(diagnosisTask({ healthSummary: null }), t)).toBeNull();
  });

  it("leaves failures to the recovery block", () => {
    const summary = "taskDiagnostics.resumeUnavailable";
    expect(detailDiagnosis(diagnosisTask({ status: "failed", healthSummary: summary }), t)).toBeNull();
    expect(detailDiagnosis(diagnosisTask({ status: "needs_attention", healthSummary: summary }), t)).toBeNull();
  });
});

describe("capability chips", () => {
  const base = { status: "downloading", protocol: "https", supportsParallel: true, supportsResume: true } as const;

  it("names range and resume support for single-file protocols", () => {
    expect(capabilityChips(base)).toEqual([
      { labelKey: "taskDetails.capability.rangeSupported", tone: "neutral" },
      { labelKey: "taskDetails.capability.resumable", tone: "neutral" },
    ]);
    expect(capabilityChips({ ...base, protocol: "sftp", supportsParallel: false, supportsResume: false })).toEqual([
      { labelKey: "taskDetails.capability.singleConnection", tone: "neutral" },
      { labelKey: "taskDetails.capability.notResumable", tone: "warning" },
    ]);
  });

  it("stays out of stream and swarm protocols and stopped tasks", () => {
    for (const protocol of ["bt", "magnet", "hls", "dash", "metalink"]) {
      expect(capabilityChips({ ...base, protocol })).toEqual([]);
    }
    // A failed resume leaves the probe's supportsResume untouched; the chip
    // would say "Resumable" right above "the server no longer supports it".
    for (const status of ["completed", "failed", "needs_attention"] as const) {
      expect(capabilityChips({ ...base, status })).toEqual([]);
    }
    expect(capabilityChips({ ...base, status: "paused" })).toHaveLength(2);
  });

  it("shows speed and ETA only while a transfer can still run", () => {
    expect(showsTransferRates("downloading")).toBe(true);
    expect(showsTransferRates("paused")).toBe(true);
    for (const status of ["completed", "failed", "needs_attention"] as const) {
      expect(showsTransferRates(status)).toBe(false);
    }
  });
});

describe("task-diagnostics helpers", () => {
  it("classifies protocols", () => {
    expect(isTorrentProtocol("bt")).toBe(true);
    expect(isTorrentProtocol("magnet")).toBe(true);
    expect(isHlsProtocol("hls")).toBe(true);
    expect(isHlsProtocol("dash")).toBe(false);
  });

  it("defaults BT diagnostics to requests", () => {
    expect(defaultDiagSubTab("bt")).toBe("requests");
    expect(defaultDiagSubTab("https")).toBe("segments");
  });

  it("picks protocol-aware empty keys", () => {
    expect(diagnosticsSegmentsEmptyKey("hls")).toBe("taskDetails.noHlsSegments");
    expect(diagnosticsSegmentsEmptyKey("ftp")).toBe("taskDetails.noWorkUnits");
    expect(diagnosticsRequestsEmptyKey("https")).toBe("taskDetails.noRequests");
    expect(diagnosticsRequestsEmptyKey("bt")).toBe("taskDetails.noRequestsGeneric");
  });

  it("gates HTTP-only request fields", () => {
    expect(showsHttpRequestFields("GET")).toBe(true);
    expect(showsHttpRequestFields("PROPFIND")).toBe(true);
    expect(showsHttpRequestFields("FTP RETR")).toBe(false);
    expect(showsHttpRequestFields("BT SOURCE")).toBe(false);
  });
});
