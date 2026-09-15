import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";
import type { IntegrityPassport } from "@/generated/bindings";
import {
  buildPassportJson,
  buildPassportTextReport,
  bytesToNumber,
  PASSPORT_CHECKSUM_STATE_KEYS,
  PASSPORT_STAGING_KEYS,
  PASSPORT_VALIDATOR_KEYS,
  TIMELINE_MILESTONE_KEYS,
  TIMELINE_TRIGGER_KEYS,
  timelineMilestones,
} from "@/lib/integrity-passport";

/**
 * The passport/timeline keys are resolved from typed tables rather than
 * literal `t("...")` calls, so `pnpm check:i18n` cannot see them. This walk
 * keeps the tables and all locales in step (same guard as backup-center-logic).
 */
describe("integrity passport key tables", () => {
  it("resolves every table key in every locale", async () => {
    const i18n = (await import("@/i18n")).default;
    const { SUPPORTED_LOCALES } = await import("@/i18n");
    const previous = i18n.language;
    try {
      for (const locale of SUPPORTED_LOCALES) {
        await i18n.changeLanguage(locale);
        for (const [state, key] of Object.entries(PASSPORT_CHECKSUM_STATE_KEYS)) {
          expect(i18n.exists(key), `${locale} missing checksumState ${state}`).toBe(true);
        }
        for (const [state, key] of Object.entries(PASSPORT_STAGING_KEYS)) {
          expect(i18n.exists(key), `${locale} missing staging ${state}`).toBe(true);
        }
        for (const [validator, key] of Object.entries(PASSPORT_VALIDATOR_KEYS)) {
          expect(i18n.exists(key), `${locale} missing validator ${validator}`).toBe(true);
        }
        for (const [trigger, key] of Object.entries(TIMELINE_TRIGGER_KEYS)) {
          expect(i18n.exists(key), `${locale} missing trigger ${trigger}`).toBe(true);
        }
        for (const [event, key] of Object.entries(TIMELINE_MILESTONE_KEYS)) {
          expect(i18n.exists(key), `${locale} missing milestone ${event}`).toBe(true);
        }
        for (const key of [
          "taskDetails.passport.title",
          "taskDetails.passport.subtitle",
          "taskDetails.passport.validatorsLabel",
          "taskDetails.passport.validatorsNone",
          "taskDetails.passport.checksumsLabel",
          "taskDetails.passport.stagingLabel",
          "taskDetails.passport.resumeStatsLabel",
          "taskDetails.passport.resumeStats",
          "taskDetails.passport.sizeLabel",
          "taskDetails.passport.sizeUnknown",
          "taskDetails.passport.loadFailed",
          "taskDetails.passport.copyReport",
          "taskDetails.passport.copyReportSuccess",
          "taskDetails.passport.copyReportFailed",
          "taskDetails.passport.exportJson",
          "taskDetails.passport.exportJsonSuccess",
          "taskDetails.passport.exportJsonFailed",
          "taskDetails.passport.computeHash",
          "taskDetails.passport.computingHash",
          "taskDetails.passport.hashComputed",
          "taskDetails.passport.hashComputeFailed",
          "taskDetails.passport.fileHashLabel",
          "taskDetails.passport.fileHashVerified",
          "taskDetails.passport.fileHashComputed",
          "taskDetails.passport.report.title",
          "taskDetails.passport.report.file",
          "taskDetails.passport.report.source",
          "taskDetails.passport.report.downloadedAt",
          "taskDetails.passport.report.bytes",
          "taskDetails.passport.report.resume",
          "taskDetails.passport.report.validators",
          "taskDetails.passport.report.validatorsNone",
          "taskDetails.passport.report.checksumNotProvided",
          "taskDetails.passport.report.checksumVerified",
          "taskDetails.passport.report.checksumFailed",
          "taskDetails.passport.report.checksumPending",
          "taskDetails.passport.report.staging",
          "taskDetails.passport.report.footer",
          "taskDetails.timeline.title",
          "taskDetails.timeline.empty",
          "taskDetails.timeline.viewAll",
          "taskEvent.needs_attention",
        ]) {
          expect(i18n.exists(key), `${locale} missing ${key}`).toBe(true);
        }
      }
    } finally {
      await i18n.changeLanguage(previous);
    }
  });
});

function makePassport(overrides: Partial<IntegrityPassport> = {}): IntegrityPassport {
  return {
    taskId: "task-1",
    fileName: "ubuntu.iso",
    sourceUrl: "https://example.com/ubuntu.iso",
    finalUrl: null,
    protocol: "https",
    taskKind: "single_file",
    status: "completed",
    totalBytes: "4700000000",
    downloadedBytes: "4700000000",
    createdAt: "2026-09-13T10:00:00.000Z",
    startedAt: "2026-09-13T10:00:01.000Z",
    completedAt: "2026-09-13T10:42:00.000Z",
    resumeCount: 2,
    segmentRetries: 3,
    supportsResume: true,
    remoteValidators: ["etag", "range"],
    checksums: [],
    checksumState: "not_provided",
    stagingCleanup: "complete",
    finalPath: "D:\\Downloads\\ubuntu.iso",
    ...overrides,
  };
}

const t = ((key: string, opts?: Record<string, unknown>) => {
  let out = key;
  if (opts) {
    for (const [name, value] of Object.entries(opts)) {
      out = out.split(`{{${name}}}`).join(String(value));
    }
  }
  // Rendering the params lets assertions observe interpolated values even
  // though the en bundle is not loaded here.
  return `${out}#${JSON.stringify(opts ?? {})}`;
}) as unknown as TFunction;

describe("buildPassportTextReport", () => {
  it("renders the honest not-provided checksum line", () => {
    const report = buildPassportTextReport(makePassport(), t);
    expect(report).toContain("taskDetails.passport.report.title");
    expect(report).toContain("taskDetails.passport.report.checksumNotProvided");
    expect(report).not.toContain("checksumVerified");
    // Paths are intentionally omitted from the shareable text report.
    expect(report).not.toContain("D:\\Downloads");
  });

  it("renders verified checksum details and validator labels", () => {
    const passport = makePassport({
      checksumState: "verified",
      checksums: [
        {
          algorithm: "sha256",
          status: "verified",
          actualHash: "deadbeef",
          verifiedAt: "2026-09-13T10:43:00.000Z",
          isPrimary: true,
          weak: false,
          sourceKind: "manual",
          errorMessage: null,
        },
      ],
    });
    const report = buildPassportTextReport(passport, t);
    expect(report).toContain("taskDetails.passport.report.checksumVerified");
    expect(report).toContain("deadbeef");
    expect(report).toContain("taskDetails.passport.validator.etag");
    expect(report).toContain("taskDetails.passport.staging.complete");
  });

  it("masks credentials embedded in the source URL", () => {
    const passport = makePassport({
      sourceUrl: "https://user:sup3rsecret@example.com/ubuntu.iso",
    });
    const report = buildPassportTextReport(passport, t);
    expect(report).not.toContain("sup3rsecret");
  });

  it("skips staging and completion lines for non-completed tasks", () => {
    const report = buildPassportTextReport(makePassport({ status: "failed", stagingCleanup: "not_applicable" }), t);
    expect(report).not.toContain("taskDetails.passport.report.staging");
  });
});

describe("buildPassportJson", () => {
  it("emits a stable snake_case envelope with schema version", () => {
    const json = buildPassportJson(makePassport(), null) as Record<string, unknown>;
    expect(json.schema_version).toBe("1");
    expect(json.checksum_state).toBe("not_provided");
    expect(json.staging_cleanup).toBe("complete");
    expect(json.remote_validators).toEqual(["etag", "range"]);
    expect(json.file_hash).toBeNull();
    const task = json.task as Record<string, unknown>;
    expect(task.file_name).toBe("ubuntu.iso");
    expect(json.timing).toBeDefined();
    expect(json.resume).toBeDefined();
  });

  it("embeds the computed file hash when provided", () => {
    const json = buildPassportJson(makePassport(), {
      algorithm: "sha256",
      digest: "cafebabe",
      source: "computed_at_export",
      computedAt: "2026-09-15T00:00:00.000Z",
    }) as Record<string, unknown>;
    expect(json.file_hash).toEqual({
      algorithm: "sha256",
      digest: "cafebabe",
      source: "computed_at_export",
      computedAt: "2026-09-15T00:00:00.000Z",
    });
  });

  it("sanitizes both stored URLs", () => {
    const json = buildPassportJson(
      makePassport({
        sourceUrl: "https://user:sup3rsecret@example.com/a.iso",
        finalUrl: "https://user:sup3rsecret@example.com/final.iso",
      }),
      null,
    ) as Record<string, unknown>;
    const task = json.task as Record<string, unknown>;
    expect(JSON.stringify(json)).not.toContain("sup3rsecret");
    expect(task.source_url).not.toContain("sup3rsecret");
    expect(task.final_url).not.toContain("sup3rsecret");
  });
});

describe("timelineMilestones", () => {
  const base = { payload: null, createdAt: "2026-07-14T00:00:00.000Z" };

  it("keeps only milestone events and maps triggers", () => {
    const milestones = timelineMilestones([
      { id: "1", eventType: "created", ...base },
      { id: "2", eventType: "bt_metadata_fetching", ...base },
      { id: "3", eventType: "started", ...base },
      { id: "4", eventType: "hash_verified", ...base },
    ]);
    expect(milestones.map((m) => m.eventType)).toEqual(["created", "started", "hash_verified"]);
    expect(milestones[0].trigger).toBe("user");
    expect(milestones[1].trigger).toBe("scheduler");
    expect(milestones[2].trigger).toBe("verification");
    expect(milestones[1].labelKey).toBe("taskEvent.started");
  });

  it("leaves ambiguous events without a trigger badge", () => {
    // `retrying` can be a user requeue or an in-flight segment error, so the
    // mapping intentionally omits it rather than guessing.
    const milestones = timelineMilestones([{ id: "1", eventType: "retrying", ...base }]);
    expect(milestones).toHaveLength(1);
    expect(milestones[0].trigger).toBeNull();
    expect(milestones[0].labelKey).toBe("taskEvent.retrying");
  });
});

describe("bytesToNumber", () => {
  it("parses IPC byte strings and degrades gracefully", () => {
    expect(bytesToNumber("1024")).toBe(1024);
    expect(bytesToNumber(512)).toBe(512);
    expect(bytesToNumber("not-a-number")).toBe(0);
    expect(bytesToNumber(null)).toBe(0);
  });
});
