import { describe, expect, it } from "vitest";

import {
  groupRecoveryConcerns,
  HISTORY_ACTION_KEYS,
  HISTORY_SOURCE_KEYS,
  isAutoRecoverable,
  playbookForTask,
  recoveryConcern,
} from "@/components/workspaces/recovery-center-logic";
import type { Task } from "@/types/task";

/**
 * The concern/playbook/history keys are resolved from tables rather than
 * literal `t("...")` calls, so `pnpm check:i18n` cannot see them. This walk
 * keeps the tables and all locales in step (same guard as storage-center and
 * environment-text).
 */
describe("recovery center key tables", () => {
  it("maps every history, source, and playbook key in every locale", async () => {
    const i18n = (await import("@/i18n")).default;
    const { SUPPORTED_LOCALES } = await import("@/i18n");
    const { PLAYBOOK } = await import("@/components/workspaces/recovery-center-logic");
    const previous = i18n.language;
    try {
      for (const locale of SUPPORTED_LOCALES) {
        await i18n.changeLanguage(locale);
        for (const [action, key] of Object.entries(HISTORY_ACTION_KEYS)) {
          expect(i18n.exists(key), `${locale} missing ${key} (action ${action})`).toBe(true);
        }
        for (const [source, key] of Object.entries(HISTORY_SOURCE_KEYS)) {
          expect(i18n.exists(key), `${locale} missing ${key} (source ${source})`).toBe(true);
        }
        for (const [action, entry] of Object.entries(PLAYBOOK)) {
          expect(i18n.exists(entry.keepsKey), `${locale} missing ${entry.keepsKey} (${action})`).toBe(true);
          expect(i18n.exists(entry.deletesKey), `${locale} missing ${entry.deletesKey} (${action})`).toBe(true);
        }
      }
    } finally {
      await i18n.changeLanguage(previous);
    }
  });
});

function makeTask(overrides: Partial<Task> & { id: string }): Task {
  return {
    url: `https://example.com/${overrides.id}`,
    finalUrl: `https://example.com/${overrides.id}`,
    protocol: "http",
    taskKind: "single_file",
    fileName: `${overrides.id}.bin`,
    saveDir: "/tmp",
    tempPath: null,
    finalPath: `/tmp/${overrides.id}.bin`,
    totalSize: 100,
    downloadedBytes: 0,
    status: "failed",
    etag: null,
    lastModified: null,
    contentType: null,
    supportsResume: true,
    supportsParallel: true,
    supportsMultiFile: false,
    sourceKey: "example.com",
    connectionCount: 0,
    speedBps: 0,
    taskSpeedLimitBps: null,
    priority: "normal",
    queuePosition: "0",
    categoryKey: null,
    obeySchedule: true,
    healthSummary: null,
    errorMessage: null,
    errorCode: null,
    recoveryActions: [],
    retryAfterAt: null,
    failureCategory: null,
    expectedHashSha256: null,
    actualHashSha256: null,
    hashStatus: "not_requested",
    hashError: null,
    hashVerifiedAt: null,
    checksums: [],
    createdAt: "2026-09-13T00:00:00Z",
    updatedAt: "2026-09-13T00:00:00Z",
    files: [],
    ...overrides,
  } as Task;
}

describe("recoveryConcern", () => {
  it("maps the backend failure categories onto user concerns", () => {
    expect(recoveryConcern(makeTask({ id: "a", failureCategory: "auth" }))).toBe("auth");
    expect(recoveryConcern(makeTask({ id: "b", failureCategory: "proxy" }))).toBe("proxy");
    expect(recoveryConcern(makeTask({ id: "c", failureCategory: "disk_write" }))).toBe("disk");
    expect(recoveryConcern(makeTask({ id: "d", failureCategory: "remote_changed" }))).toBe("remoteChanged");
    expect(recoveryConcern(makeTask({ id: "e", failureCategory: "resume_unavailable" }))).toBe("resume");
    expect(recoveryConcern(makeTask({ id: "f", failureCategory: "temp_file" }))).toBe("resume");
    expect(recoveryConcern(makeTask({ id: "g", failureCategory: "bt" }))).toBe("protocol");
    expect(recoveryConcern(makeTask({ id: "h", failureCategory: "sftp" }))).toBe("protocol");
    expect(recoveryConcern(makeTask({ id: "i", failureCategory: "http" }))).toBe("http");
    expect(recoveryConcern(makeTask({ id: "j", failureCategory: "schedule" }))).toBe("other");
  });

  it("lets structured error codes override the category", () => {
    expect(
      recoveryConcern(makeTask({ id: "a", failureCategory: "http", errorCode: "task_credentials_unavailable" })),
    ).toBe("auth");
    expect(recoveryConcern(makeTask({ id: "b", failureCategory: "other", errorCode: "ffmpeg_missing" }))).toBe(
      "protocol",
    );
    expect(recoveryConcern(makeTask({ id: "c", failureCategory: "other", errorCode: "disk_write_failed" }))).toBe(
      "disk",
    );
  });

  it("falls back to the recovery actions when no category is present", () => {
    expect(recoveryConcern(makeTask({ id: "a", recoveryActions: ["manage_sftp_host_keys"] }))).toBe("auth");
    expect(recoveryConcern(makeTask({ id: "b", recoveryActions: ["free_disk_space"] }))).toBe("disk");
    expect(recoveryConcern(makeTask({ id: "c", recoveryActions: ["check_url"] }))).toBe("remoteChanged");
    expect(recoveryConcern(makeTask({ id: "d", recoveryActions: ["open_folder"] }))).toBe("other");
  });
});

describe("isAutoRecoverable", () => {
  it("matches the backend bulk gate", () => {
    expect(isAutoRecoverable(makeTask({ id: "a", status: "failed" }))).toBe(true);
    expect(isAutoRecoverable(makeTask({ id: "b", status: "needs_attention" }))).toBe(true);
    expect(isAutoRecoverable(makeTask({ id: "c", status: "queued" }))).toBe(false);
    expect(isAutoRecoverable(makeTask({ id: "d", status: "needs_attention", errorCode: "remote_changed" }))).toBe(
      false,
    );
    expect(
      isAutoRecoverable(makeTask({ id: "e", status: "needs_attention", errorCode: "temp_file_smaller_than_progress" })),
    ).toBe(false);
    // A publish-path conflict persists until the user acts (backend gate
    // parity) — bulk retry would just re-fail at publish.
    expect(isAutoRecoverable(makeTask({ id: "f", status: "needs_attention", errorCode: "final_path_conflict" }))).toBe(
      false,
    );
  });
});

describe("groupRecoveryConcerns", () => {
  it("counts auto vs needs-input and leads with the most fixable group", () => {
    const groups = groupRecoveryConcerns([
      makeTask({ id: "http-1", failureCategory: "http", status: "failed" }),
      makeTask({ id: "http-2", failureCategory: "http", status: "failed" }),
      makeTask({
        id: "changed-1",
        failureCategory: "remote_changed",
        errorCode: "remote_changed",
        status: "needs_attention",
      }),
      makeTask({ id: "auth-1", failureCategory: "auth", status: "failed" }),
    ]);

    expect(groups.map((group) => group.concern)).toEqual(["http", "auth", "remoteChanged"]);
    const http = groups[0];
    expect(http.autoRecoverable).toBe(2);
    expect(http.needsInput).toBe(0);
    const changed = groups.find((group) => group.concern === "remoteChanged");
    expect(changed?.autoRecoverable).toBe(0);
    expect(changed?.needsInput).toBe(1);
  });
});

describe("playbookForTask", () => {
  it("orders the actions safest first and documents consequences", () => {
    const playbook = playbookForTask(makeTask({ id: "a", recoveryActions: ["restart", "check_url", "retry"] }));
    expect(playbook.map((entry) => entry.action)).toEqual(["retry", "check_url", "restart"]);
    const restart = playbook[2];
    expect(restart.redownloads).toBe(true);
    expect(restart.changesPath).toBe(false);
    const retry = playbook[0];
    expect(retry.redownloads).toBe(false);
  });

  it("falls back to error-derived actions when none are persisted", () => {
    const playbook = playbookForTask(
      makeTask({
        id: "a",
        errorMessage: JSON.stringify({
          code: "final_path_conflict",
          message: "target exists",
          recoverable: false,
          actions: ["choose_another_name", "choose_another_folder", "retry"],
        }),
      }),
    );
    expect(playbook.map((entry) => entry.action)).toEqual(["retry", "choose_another_name", "choose_another_folder"]);
    expect(playbook[1].changesPath).toBe(true);
  });
});
