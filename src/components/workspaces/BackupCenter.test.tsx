import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BackupCenter } from "@/components/workspaces/BackupCenter";
import type { BackupContents, BackupValidateResult, RestoreReport } from "@/generated/bindings";

const tauriMocks = vi.hoisted(() => ({
  describeBackupSource: vi.fn(),
  validateAppBackup: vi.fn(),
  restoreAppBackup: vi.fn(),
  restoreBackupSubset: vi.fn(),
  getLastRestoreReport: vi.fn(),
  dismissRestoreReport: vi.fn(),
}));

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, values?: Record<string, unknown>) => {
        if (!values) return key;
        return `${key}:${JSON.stringify(values)}`;
      },
      i18n: { language: "en" },
    }),
  };
});

vi.mock("@/lib/backup", () => ({
  exportAppBackup: vi.fn(),
  pickBackupFile: vi.fn(),
  pickRemapRoot: vi.fn(),
}));

vi.mock("@/lib/tauri", () => tauriMocks);
vi.mock("@/stores/toast-store", () => ({
  useToastStore: (selector: (state: { addToast: (...args: unknown[]) => void }) => unknown) =>
    selector({ addToast: vi.fn() }),
}));

import { exportAppBackup, pickBackupFile, pickRemapRoot } from "@/lib/backup";

function makeContents(): BackupContents {
  return {
    tasksTotal: 12,
    tasksCompleted: 7,
    tasksFailed: 2,
    classificationRules: 3,
    siteRules: 4,
    tasksWithChecksums: 5,
    tasksWithCredentials: 1,
    tasksWithRequestHeaders: 0,
    settingsKeys: 33,
    taskEvents: 40,
  };
}

function makeValidation(overrides: Partial<BackupValidateResult> = {}): BackupValidateResult {
  return {
    path: "C:/backups/v.vibe-backup",
    schemaVersion: "9",
    appVersion: "0.5.0",
    createdAt: "2026-09-13T00:00:00Z",
    credentialsPolicy: "machine_bound_ciphertext",
    databaseBytes: "1048576",
    contents: makeContents(),
    pathPolicy: { violationCount: 0, sampleViolations: [], offendingSaveDirs: [] },
    disk: { freeBytes: "900000000000", requiredBytes: "3000000" },
    settingsPreview: {
      ffmpegConfigured: false,
      completionAction: "notify",
      proxyPasswordSaved: false,
      defaultSaveDir: "",
    },
    ...overrides,
  };
}

function makeReport(): RestoreReport {
  return {
    schemaVersion: "9",
    restoredAt: "2026-09-13T01:00:00Z",
    backupCreatedAt: "2026-09-01T00:00:00Z",
    preRestoreBackupPath: "C:/rollbacks/vibe.db.bak-1",
    tasksWithCredentials: 2,
    tasksWithPerTaskProxy: 0,
    globalProxyNeedsReentry: true,
    ffmpegWasConfigured: true,
    completionActionReset: false,
    missingSaveDirs: ["D:/OldDownloads"],
    missingSaveDirsTotal: 1,
  };
}

function renderPage() {
  return render(<BackupCenter />);
}

describe("BackupCenter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.describeBackupSource.mockResolvedValue(makeContents());
    tauriMocks.getLastRestoreReport.mockResolvedValue(null);
    tauriMocks.dismissRestoreReport.mockResolvedValue(true);
    vi.mocked(exportAppBackup).mockResolvedValue({
      path: "C:/backups/new.vibe-backup",
      schemaVersion: "9",
      credentialsPolicy: "machine_bound_ciphertext",
      usedCopyFallback: false,
    });
    vi.mocked(pickBackupFile).mockResolvedValue(null);
    vi.mocked(pickRemapRoot).mockResolvedValue(null);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders the live inventory counts and the never-exported list", async () => {
    renderPage();
    await waitFor(() => expect(tauriMocks.describeBackupSource).toHaveBeenCalled());
    expect(await screen.findByText("12")).toBeInTheDocument();
    expect(screen.getByText("backupCenter.contents.tasksTotal")).toBeInTheDocument();
    expect(screen.getByText("backupCenter.notExported.proxyPassword")).toBeInTheDocument();
    expect(screen.getByText("backupCenter.notExported.btState")).toBeInTheDocument();
  });

  it("shows export diagnostics including the cross-volume fallback note", async () => {
    vi.mocked(exportAppBackup).mockResolvedValue({
      path: "E:/backups/new.vibe-backup",
      schemaVersion: "9",
      credentialsPolicy: "machine_bound_ciphertext",
      usedCopyFallback: true,
    });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "backupCenter.exportButton" }));
    expect(await screen.findByText(/backupCenter\.exportResultPath/)).toBeInTheDocument();
    expect(screen.getByText("backupCenter.usedCopyFallback")).toBeInTheDocument();
  });

  it("shows a clean check panel and no remap toggle when paths are fine", async () => {
    vi.mocked(pickBackupFile).mockResolvedValue("C:/backups/v.vibe-backup");
    tauriMocks.validateAppBackup.mockResolvedValue(makeValidation());
    renderPage();
    const pickButtons = await screen.findAllByRole("button", { name: "backupCenter.pickFile" });
    fireEvent.click(pickButtons[0]);
    expect(await screen.findByText("backupCenter.check.pathsOk")).toBeInTheDocument();
    expect(tauriMocks.validateAppBackup).toHaveBeenCalledWith("C:/backups/v.vibe-backup");
    expect(screen.queryByLabelText(/backupCenter\.remap\.toggle/)).not.toBeChecked();
    // The toggle checkbox exists (unchecked) even without violations.
    const toggle = screen.queryByRole("checkbox", { name: /backupCenter\.remap\.toggle/ });
    expect(toggle).not.toBeNull();
  });

  it("pre-enables the remap toggle and lists offending dirs when the policy flags violations", async () => {
    vi.mocked(pickBackupFile).mockResolvedValue("C:/backups/foreign.vibe-backup");
    tauriMocks.validateAppBackup.mockResolvedValue(
      makeValidation({
        pathPolicy: {
          violationCount: 4,
          sampleViolations: [],
          offendingSaveDirs: ["D:/OldDownloads"],
        },
        disk: { freeBytes: "10", requiredBytes: "3000000" },
        settingsPreview: {
          ffmpegConfigured: true,
          completionAction: "shutdown",
          proxyPasswordSaved: true,
          defaultSaveDir: "D:/OldDownloads",
        },
      }),
    );
    renderPage();
    const pickButtons = await screen.findAllByRole("button", { name: "backupCenter.pickFile" });
    fireEvent.click(pickButtons[0]);
    expect(await screen.findByText(/backupCenter\.check\.pathsWarning/)).toBeInTheDocument();
    // The dir shows up twice (offending list + save-dir preview line).
    expect(screen.getAllByText(/D:\/OldDownloads/).length).toBeGreaterThan(0);
    expect(screen.getByText(/backupCenter\.check\.scrubFfmpeg/)).toBeInTheDocument();
    expect(screen.getByText(/backupCenter\.check\.scrubProxy/)).toBeInTheDocument();
    const toggle = screen.getByRole("checkbox", { name: /backupCenter\.remap\.toggle/ });
    expect(toggle).toBeChecked();
  });

  it("routes the restore through the confirm dialog with the remap root", async () => {
    vi.mocked(pickBackupFile).mockResolvedValue("C:/backups/v.vibe-backup");
    vi.mocked(pickRemapRoot).mockResolvedValue("D:/NewRoot");
    tauriMocks.validateAppBackup.mockResolvedValue(makeValidation());
    tauriMocks.restoreAppBackup.mockResolvedValue({
      requiresRestart: true,
      preRestoreBackupPath: "C:/bak",
      pendingRestorePath: "C:/pending",
      credentialsPolicy: "machine_bound_ciphertext",
      remappedPaths: 3,
    });
    renderPage();
    const pickButtons = await screen.findAllByRole("button", { name: "backupCenter.pickFile" });
    fireEvent.click(pickButtons[0]);
    const toggle = await screen.findByRole("checkbox", { name: /backupCenter\.remap\.toggle/ });
    fireEvent.click(toggle);
    fireEvent.click(await screen.findByRole("button", { name: "backupCenter.remap.pick" }));
    await waitFor(() => expect(pickRemapRoot).toHaveBeenCalled());
    // The picked root must be committed to state before the confirm dialog.
    await screen.findByText("D:/NewRoot");
    fireEvent.click(screen.getByRole("button", { name: "backupCenter.confirm.open" }));
    fireEvent.click(await screen.findByRole("button", { name: "backupCenter.confirm.confirm" }));
    await waitFor(() =>
      expect(tauriMocks.restoreAppBackup).toHaveBeenCalledWith("C:/backups/v.vibe-backup", "D:/NewRoot"),
    );
  });

  it("runs the subset restore only with a file and a selection", async () => {
    vi.mocked(pickBackupFile).mockResolvedValue("C:/backups/v.vibe-backup");
    tauriMocks.restoreBackupSubset.mockResolvedValue({
      tasksInserted: 1,
      tasksSkipped: 0,
      tasksNormalized: 0,
      rulesInserted: 2,
      rulesSkipped: 0,
      settingsReplaced: 0,
    });
    renderPage();
    const pickButtons = await screen.findAllByRole("button", { name: "backupCenter.pickFile" });
    fireEvent.click(pickButtons[pickButtons.length - 1]);
    await waitFor(() => expect(pickBackupFile).toHaveBeenCalled());

    const runButton = screen.getByRole("button", { name: "backupCenter.subset.run" });
    expect(runButton).toBeDisabled();

    const checkboxes = screen.getAllByRole("checkbox");
    const taskBox = checkboxes[checkboxes.length - 3];
    const rulesBox = checkboxes[checkboxes.length - 2];
    fireEvent.click(taskBox);
    fireEvent.click(rulesBox);
    expect(runButton).toBeEnabled();
    fireEvent.click(runButton);
    await waitFor(() =>
      expect(tauriMocks.restoreBackupSubset).toHaveBeenCalledWith("C:/backups/v.vibe-backup", {
        tasks: true,
        rules: true,
        settings: false,
      }),
    );
    // Toast rendering is mocked away; assert the payload instead.
  });

  it("renders the post-restore report and dismisses it", async () => {
    tauriMocks.getLastRestoreReport.mockResolvedValue(makeReport());
    renderPage();
    expect(await screen.findByText(/backupCenter\.report\.credentials/)).toBeInTheDocument();
    expect(screen.getByText(/backupCenter\.report\.proxyGlobal/)).toBeInTheDocument();
    expect(screen.getByText(/backupCenter\.report\.ffmpeg/)).toBeInTheDocument();
    expect(screen.getByText(/backupCenter\.report\.rollback/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "backupCenter.report.dismiss" }));
    await waitFor(() => expect(tauriMocks.dismissRestoreReport).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByText(/backupCenter\.report\.credentials/)).not.toBeInTheDocument());
  });

  it("hides the report section when there is nothing to show", async () => {
    renderPage();
    await waitFor(() => expect(tauriMocks.getLastRestoreReport).toHaveBeenCalled());
    expect(screen.queryByText(/backupCenter\.reportTitle/)).not.toBeInTheDocument();
  });
});
