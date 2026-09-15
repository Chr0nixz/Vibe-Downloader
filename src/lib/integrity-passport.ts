//! Pure helpers for the Integrity Passport (feature proposal §2.4): typed
//! label tables, the sanitized clipboard report, and the stable JSON export.
//!
//! Honesty rule: a task with no configured checksum must render explicit
//! "not provided" copy — never a green "verified" and never a blank section.
//! The backend cannot know the active language, so every label resolves
//! through i18n here; raw digests and URLs stay untranslated (UX-11).

import type { TFunction } from "i18next";

import type {
  IntegrityPassport,
  PassportChecksumState,
  PassportStagingCleanup,
  RemoteValidatorKind,
} from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";
import { formatDateTime } from "@/lib/format-date";
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";

/** Byte counts cross IPC as strings (Specta convention); degrade to 0. */
export function bytesToNumber(value: string | number | null | undefined): number {
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  if (!value) return 0;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export const PASSPORT_CHECKSUM_STATE_KEYS = {
  verified: "taskDetails.passport.checksumState.verified",
  failed: "taskDetails.passport.checksumState.failed",
  pending: "taskDetails.passport.checksumState.pending",
  not_provided: "taskDetails.passport.checksumState.notProvided",
} as const satisfies Record<PassportChecksumState, TranslationKey>;

export const PASSPORT_STAGING_KEYS = {
  complete: "taskDetails.passport.staging.complete",
  incomplete: "taskDetails.passport.staging.incomplete",
  missing_output: "taskDetails.passport.staging.missingOutput",
  not_applicable: "taskDetails.passport.staging.notApplicable",
} as const satisfies Record<PassportStagingCleanup, TranslationKey>;

export const PASSPORT_VALIDATOR_KEYS = {
  etag: "taskDetails.passport.validator.etag",
  last_modified: "taskDetails.passport.validator.lastModified",
  range: "taskDetails.passport.validator.range",
} as const satisfies Record<RemoteValidatorKind, TranslationKey>;

/** Who caused a timeline milestone. Inferred from the event vocabulary —
 * ambiguous events (e.g. `retrying` can be a user requeue or a segment
 * error) get no badge instead of a guess. */
export type TimelineTrigger = "user" | "scheduler" | "remote" | "error" | "engine" | "verification";

export const TIMELINE_TRIGGER_KEYS = {
  user: "taskDetails.timeline.trigger.user",
  scheduler: "taskDetails.timeline.trigger.scheduler",
  remote: "taskDetails.timeline.trigger.remote",
  error: "taskDetails.timeline.trigger.error",
  engine: "taskDetails.timeline.trigger.engine",
  verification: "taskDetails.timeline.trigger.verification",
} as const satisfies Record<TimelineTrigger, TranslationKey>;

/** Milestone events shown in the Overview timeline card; everything else
 * stays in the Logs tab. Values reuse the existing `taskEvent.*` labels. */
export const TIMELINE_MILESTONE_KEYS = {
  created: "taskEvent.created",
  started: "taskEvent.started",
  paused: "taskEvent.paused",
  paused_by_schedule: "taskEvent.paused_by_schedule",
  resumed: "taskEvent.resumed",
  retrying: "taskEvent.retrying",
  failed: "taskEvent.failed",
  needs_attention: "taskEvent.needs_attention",
  completed: "taskEvent.completed",
  hash_verified: "taskEvent.hash_verified",
  hash_failed: "taskEvent.hash_failed",
  resume_blocked: "taskEvent.resume_blocked",
  resume_checked: "taskEvent.resume_checked",
  checksums_discovered: "taskEvent.checksums_discovered",
} as const satisfies Record<string, TranslationKey>;

export const TIMELINE_EVENT_TRIGGERS: Partial<Record<keyof typeof TIMELINE_MILESTONE_KEYS, TimelineTrigger>> = {
  created: "user",
  started: "scheduler",
  paused: "user",
  paused_by_schedule: "scheduler",
  resumed: "user",
  failed: "error",
  needs_attention: "error",
  resume_blocked: "error",
  completed: "engine",
  hash_verified: "verification",
  hash_failed: "verification",
  checksums_discovered: "verification",
};

export type TimelineMilestone = {
  id: string;
  eventType: string;
  labelKey: TranslationKey | null;
  trigger: TimelineTrigger | null;
  payload: string | null;
  createdAt: string;
};

/** Filter the event log down to milestones, newest first (the page query
 * already returns that order, matching the Logs tab). */
export function timelineMilestones(
  events: { id: string; eventType: string; payload: string | null; createdAt: string }[],
): TimelineMilestone[] {
  return events
    .filter((event) => event.eventType in TIMELINE_MILESTONE_KEYS)
    .map((event) => ({
      id: event.id,
      eventType: event.eventType,
      labelKey: TIMELINE_MILESTONE_KEYS[event.eventType as keyof typeof TIMELINE_MILESTONE_KEYS] ?? null,
      trigger: TIMELINE_EVENT_TRIGGERS[event.eventType as keyof typeof TIMELINE_EVENT_TRIGGERS] ?? null,
      payload: event.payload,
      createdAt: event.createdAt,
    }));
}

/** Sanitized multi-line clipboard report. Paths are intentionally omitted —
 * the report is the shareable summary; the JSON export carries paths. */
export function buildPassportTextReport(passport: IntegrityPassport, t: TFunction): string {
  const lines: string[] = [t("taskDetails.passport.report.title")];

  lines.push(t("taskDetails.passport.report.file", { name: passport.fileName }));
  lines.push(t("taskDetails.passport.report.source", { url: sanitizeUrlForDisplay(passport.sourceUrl) }));
  if (passport.completedAt) {
    lines.push(
      t("taskDetails.passport.report.downloadedAt", {
        time: formatDateTime(passport.completedAt, "dateTime"),
      }),
    );
  }
  if (passport.totalBytes) {
    lines.push(t("taskDetails.passport.report.bytes", { bytes: formatBytes(bytesToNumber(passport.totalBytes)) }));
  }
  lines.push(
    t("taskDetails.passport.report.resume", {
      resumes: passport.resumeCount,
      retries: passport.segmentRetries,
    }),
  );

  if (passport.remoteValidators.length > 0) {
    lines.push(
      t("taskDetails.passport.report.validators", {
        validators: passport.remoteValidators.map((validator) => t(PASSPORT_VALIDATOR_KEYS[validator])).join(", "),
      }),
    );
  } else {
    lines.push(t("taskDetails.passport.report.validatorsNone"));
  }

  if (passport.checksumState === "not_provided") {
    lines.push(t("taskDetails.passport.report.checksumNotProvided"));
  } else {
    for (const checksum of passport.checksums) {
      const algorithm = checksum.algorithm.toUpperCase();
      if (checksum.status === "verified") {
        const verifiedAt = checksum.verifiedAt ? formatDateTime(checksum.verifiedAt, "dateTime") : "";
        lines.push(
          t("taskDetails.passport.report.checksumVerified", {
            algorithm,
            digest: checksum.actualHash ?? "",
            time: verifiedAt,
          }),
        );
      } else if (checksum.status === "failed") {
        lines.push(t("taskDetails.passport.report.checksumFailed", { algorithm }));
      } else {
        lines.push(t("taskDetails.passport.report.checksumPending", { algorithm }));
      }
    }
  }

  if (passport.status === "completed") {
    lines.push(t("taskDetails.passport.report.staging", { state: t(PASSPORT_STAGING_KEYS[passport.stagingCleanup]) }));
  }

  lines.push("");
  lines.push(t("taskDetails.passport.report.footer"));
  return lines.join("\n");
}

export const PASSPORT_JSON_SCHEMA_VERSION = "1";

/** Hash of the downloaded file embedded in the JSON export. `verified` means
 * the digest comes from a persisted verification; `computed_at_export` means
 * it was hashed on demand and is not persisted anywhere. */
export type PassportFileHash = {
  algorithm: string;
  digest: string;
  source: "verified" | "computed_at_export";
  computedAt: string;
};

/** Stable snake_case export for scripts/archives. `schemaVersion` lets
 * consumers detect format changes; unknown fields must be tolerated. */
export function buildPassportJson(
  passport: IntegrityPassport,
  fileHash: PassportFileHash | null,
): Record<string, unknown> {
  return {
    schema_version: PASSPORT_JSON_SCHEMA_VERSION,
    exported_at: new Date().toISOString(),
    task: {
      id: passport.taskId,
      file_name: passport.fileName,
      // sanitizeUrlForDisplay masks basic-auth credentials; task URLs are
      // already credential-free at creation (FUN-01), this is belt and braces.
      source_url: sanitizeUrlForDisplay(passport.sourceUrl),
      final_url: passport.finalUrl ? sanitizeUrlForDisplay(passport.finalUrl) : null,
      protocol: passport.protocol,
      task_kind: passport.taskKind,
      status: passport.status,
    },
    bytes: {
      total: passport.totalBytes,
      downloaded: passport.downloadedBytes,
    },
    timing: {
      created_at: passport.createdAt,
      started_at: passport.startedAt,
      completed_at: passport.completedAt,
    },
    resume: {
      resumes: passport.resumeCount,
      segment_retries: passport.segmentRetries,
      supports_resume: passport.supportsResume,
    },
    remote_validators: passport.remoteValidators,
    checksum_state: passport.checksumState,
    checksums: passport.checksums.map((checksum) => ({
      algorithm: checksum.algorithm,
      status: checksum.status,
      actual_hash: checksum.actualHash,
      verified_at: checksum.verifiedAt,
      is_primary: checksum.isPrimary,
      weak: checksum.weak,
      source_kind: checksum.sourceKind,
      error: checksum.errorMessage,
    })),
    staging_cleanup: passport.stagingCleanup,
    final_path: passport.finalPath,
    file_hash: fileHash,
  };
}
