import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronDown,
  File,
  FileArchive,
  FileAudio,
  FileImage,
  FileText,
  FileVideo,
  FolderOpen,
  ListPlus,
  Pencil,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type {
  BatchImportResult,
  FtpDirectoryProbe,
  ProbeTaskPayload,
  ProbedFile,
} from "@/generated/bindings";
import { createLogger } from "@/lib/logger";
import { errorMessage, parseAppError } from "@/lib/errors";
import {
  createTask,
  importUrls,
  openDirectoryPicker,
  openFilePicker,
  probeFtpDirectory,
  probeTask,
} from "@/lib/tauri";

const log = createLogger("new-download");
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { parseByteCount } from "@/types/task";
import { normalizeTask } from "@/types/task";
import type { Task } from "@/types/task";

/* ------------------------------------------------------------------ */
/*  Local file picker types                                            */
/* ------------------------------------------------------------------ */

interface SelectedLocalFile {
  path: string;
  name: string;
  kind: "torrent" | "metalink" | "text";
}

function getLocalFileKind(name: string): SelectedLocalFile["kind"] {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "torrent") return "torrent";
  if (ext === "meta4" || ext === "metalink") return "metalink";
  return "text";
}

function readFileAsText(filePath: string): Promise<string> {
  return import("@tauri-apps/plugin-fs").then(({ readTextFile }) =>
    readTextFile(filePath),
  );
}

function encodePathSegments(path: string): string {
  return path.split("/").map((segment) => encodeURIComponent(segment)).join("/");
}

function pathToFileUrl(filePath: string): string {
  const normalized = filePath.replace(/\\/g, "/");
  if (/^[A-Za-z]:\//.test(normalized)) {
    const drive = normalized.slice(0, 2);
    return `file:///${drive}${encodePathSegments(normalized.slice(2))}`;
  }
  if (normalized.startsWith("//")) {
    const [host = "", ...segments] = normalized.slice(2).split("/");
    return `file://${encodeURIComponent(host)}/${segments.map((segment) => encodeURIComponent(segment)).join("/")}`;
  }
  return `file://${encodePathSegments(normalized.startsWith("/") ? normalized : `/${normalized}`)}`;
}

function localFileKindLabel(kind: SelectedLocalFile["kind"], t: (key: string) => string): string {
  if (kind === "torrent") return t("newDownload.fileKindTorrent");
  if (kind === "metalink") return t("newDownload.fileKindMetalink");
  return t("newDownload.fileKindText");
}

/* ------------------------------------------------------------------ */
/*  File icon helper                                                    */
/* ------------------------------------------------------------------ */

function fileIcon(name: string, className = "h-4 w-4") {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst"].includes(ext))
    return <FileArchive className={className} />;
  if (["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "ts"].includes(ext))
    return <FileVideo className={className} />;
  if (["mp3", "flac", "wav", "aac", "ogg", "m4a", "opus"].includes(ext))
    return <FileAudio className={className} />;
  if (["jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "ico"].includes(ext))
    return <FileImage className={className} />;
  if (["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "epub"].includes(ext))
    return <FileText className={className} />;
  return <File className={className} />;
}

/* ------------------------------------------------------------------ */
/*  Main component                                                      */
/* ------------------------------------------------------------------ */

export function NewDownloadDialog({
  open,
  onOpenChange,
  onCreated,
  initialUrl,
  initialBatchInput,
  initialSourceId,
  onDraftStateChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (task: Task) => void;
  initialUrl?: string;
  initialBatchInput?: string;
  initialSourceId?: string;
  onDraftStateChange?: (dirty: boolean) => void;
}) {
  const { t } = useTranslation();
  const settings = useSettingsStore((s) => s.settings);
  const [url, setUrl] = useState("");
  const [saveDir, setSaveDir] = useState("");
  const [fileName, setFileName] = useState("");
  const [expectedHashSha256, setExpectedHashSha256] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<ProbeTaskPayload | null>(null);
  const [probeUrl, setProbeUrl] = useState("");
  const [batchInput, setBatchInput] = useState("");
  const [batchResult, setBatchResult] = useState<BatchImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ftpDirectoryProbe, setFtpDirectoryProbe] = useState<FtpDirectoryProbe | null>(null);
  const [ftpDirectoryLoading, setFtpDirectoryLoading] = useState(false);
  const [duplicateOverrideAvailable, setDuplicateOverrideAvailable] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<string | null>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [selectedLocalFile, setSelectedLocalFile] = useState<SelectedLocalFile | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [editingName, setEditingName] = useState(false);

  // Multi-file selection: set of selected indices from probe.files
  const [selectedFiles, setSelectedFiles] = useState<Set<number>>(new Set());

  const probeRequestId = useRef(0);
  const batchInputRef = useRef<HTMLTextAreaElement | null>(null);
  const appliedInitialSourceId = useRef<string | undefined>(undefined);
  const isTorrentProbe = probe?.protocol === "bt" || probe?.protocol === "magnet";
  const isMetalinkProbe = probe?.protocol === "metalink";
  const isHlsProbe = probe?.protocol === "hls";
  const isSelectableMultiFileProbe = isTorrentProbe || isMetalinkProbe;
  const isMultiFile = probe != null && probe.files.length > 1;
  const shouldShowManifestProtocolHint =
    isTorrentProbe ||
    isMetalinkProbe ||
    selectedLocalFile?.kind === "torrent" ||
    selectedLocalFile?.kind === "metalink" ||
    /\.(torrent|meta4|metalink)(?:[?#].*)?$/i.test(url.trim());
  const fileSelectionRequired =
    isSelectableMultiFileProbe && isMultiFile && selectedFiles.size === 0;
  const canProbeFtpDirectory = /^(ftp|ftps):\/\//i.test(url.trim()) && url.trim().endsWith("/");

  // Initialize selectedFiles when probe changes
  useEffect(() => {
    if (probe && probe.files.length > 1) {
      setSelectedFiles(new Set(probe.files.map((_, i) => i)));
    } else {
      setSelectedFiles(new Set());
    }
  }, [probe]);

  async function detect(nextUrl = url.trim(), automatic = false) {
    if (!nextUrl) return;
    const requestId = ++probeRequestId.current;
    setProbing(true);
    setDuplicateOverrideAvailable(false);
    if (!automatic) setError(null);
    setProbe(null);
    setProbeUrl("");
    setFtpDirectoryProbe(null);
    try {
      const nextProbe = await probeTask({ url: nextUrl });
      if (requestId !== probeRequestId.current) return;
      setProbe(nextProbe);
      setProbeUrl(nextUrl);
      if (!fileName.trim()) {
        setFileName(nextProbe.fileName);
      }
      setError(null);
    } catch (err) {
      if (requestId !== probeRequestId.current) return;
      log.warn("probe failed", err);
      setError(errorMessage(err));
    } finally {
      if (requestId === probeRequestId.current) setProbing(false);
    }
  }

  async function runFtpDirectoryProbe() {
    setFtpDirectoryLoading(true);
    setError(null);
    try {
      setFtpDirectoryProbe(await probeFtpDirectory(url.trim()));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setFtpDirectoryLoading(false);
    }
  }

  useEffect(() => {
    const nextUrl = url.trim();
    setSubmitStatus(null);
    setDuplicateOverrideAvailable(false);
    if (!nextUrl) {
      setProbe(null);
      setProbeUrl("");
      setError(null);
      return;
    }

    const timeoutId = window.setTimeout(() => {
      void detect(nextUrl, true);
    }, 650);

    return () => window.clearTimeout(timeoutId);
  }, [url]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await submitCurrent(false);
  }

  async function submitDuplicateOverride() {
    await submitCurrent(true);
  }

  async function submitCurrent(allowDuplicate: boolean) {
    const currentUrl = url.trim();
    const currentProbe = probe && probeUrl === currentUrl ? probe : null;
    const currentIsTorrentProbe =
      currentProbe?.protocol === "bt" || currentProbe?.protocol === "magnet";
    const currentIsMetalinkProbe = currentProbe?.protocol === "metalink";
    const currentIsHlsProbe = currentProbe?.protocol === "hls";
    const currentIsSelectableMultiFileProbe = currentIsTorrentProbe || currentIsMetalinkProbe;
    const selectedFilePaths =
      currentProbe && currentIsSelectableMultiFileProbe && currentProbe.files.length > 1
        ? Array.from(selectedFiles)
            .sort((left, right) => left - right)
            .map((index) => currentProbe.files[index]?.relativePath)
            .filter((path): path is string => Boolean(path))
        : null;
    if (
      currentProbe &&
      currentIsSelectableMultiFileProbe &&
      currentProbe.files.length > 1 &&
      selectedFilePaths?.length === 0
    ) {
      setError(t("newDownload.fileSelectionRequired"));
      setSubmitStatus(null);
      return;
    }

    setSubmitting(true);
    setError(null);
    setDuplicateOverrideAvailable(false);
    setSubmitStatus(
      currentProbe
        ? t("newDownload.usingProbe")
        : t("newDownload.revalidating"),
    );
    try {
      const task = await createTask({
        url: currentUrl,
        saveDir: saveDir.trim() || null,
        fileName: fileName.trim() || null,
        expectedHashSha256:
          currentIsTorrentProbe || currentIsMetalinkProbe || currentIsHlsProbe
            ? null
            : expectedHashSha256.trim() || null,
        taskSpeedLimitBps: null,
        priority: null,
        categoryKey: null,
        probeSnapshot: currentProbe,
        selectedFilePaths,
        allowDuplicate,
      });
      onCreated(task);
      resetForm();
      onOpenChange(false);
    } catch (err) {
      log.error("create task failed", err);
      const appError = parseAppError(err);
      setDuplicateOverrideAvailable(appError?.code === "duplicate_task");
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
      setSubmitStatus(null);
    }
  }

  function resetForm() {
    setUrl("");
    setSaveDir("");
    setFileName("");
    setExpectedHashSha256("");
    setProbe(null);
    setProbeUrl("");
    setBatchResult(null);
    setDuplicateOverrideAvailable(false);
    setSubmitStatus(null);
    setAdvancedOpen(false);
    setSelectedLocalFile(null);
    setSelectedFiles(new Set());
    setEditingName(false);
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setSaveDir(selected);
  }

  async function chooseLocalFile() {
    setFileLoading(true);
    try {
      const picked = await openFilePicker([
        { name: "Manifest / Text", extensions: ["torrent", "meta4", "metalink", "txt"] },
      ]);
      if (!picked) return;

      const kind = getLocalFileKind(picked.name);
      const file: SelectedLocalFile = { path: picked.path, name: picked.name, kind };
      setSelectedLocalFile(file);

      if (kind === "torrent" || kind === "metalink") {
        const fileUrl = pathToFileUrl(picked.path);
        setUrl(fileUrl);
        setProbe(null);
        setProbeUrl("");
      } else {
        try {
          const text = await readFileAsText(picked.path);
          setBatchInput(text);
          setAdvancedOpen(true);
        } catch (err) {
          log.warn("failed to read text file", err);
          setError(errorMessage(err));
        }
      }
    } catch (err) {
      log.error("file picker failed", err);
      setError(errorMessage(err));
    } finally {
      setFileLoading(false);
    }
  }

  function clearSelectedLocalFile() {
    setSelectedLocalFile(null);
    if (selectedLocalFile?.kind === "torrent" || selectedLocalFile?.kind === "metalink") {
      setUrl("");
      setProbe(null);
      setProbeUrl("");
    }
  }

  async function runBatch(create: boolean, inputOverride?: string) {
    const input = inputOverride ?? batchInput;
    if (!input.trim()) return;
    setSubmitting(create);
    setError(null);
    try {
      const result = await importUrls({
        input,
        saveDir: saveDir.trim() || null,
        probe: true,
        create,
      });
      setBatchResult(result);
      for (const item of result.items) {
        if (item.task) onCreated(normalizeTask(item.task));
      }
      if (create && result.createdCount > 0) {
        setBatchInput("");
      }
    } catch (err) {
      log.error("batch import failed", err);
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
    }
  }

  function openBatchImport() {
    setAdvancedOpen(true);
    window.setTimeout(() => batchInputRef.current?.focus(), 0);
  }

  useEffect(() => {
    if (!initialSourceId || appliedInitialSourceId.current === initialSourceId) return;
    appliedInitialSourceId.current = initialSourceId;
    resetForm();

    const nextBatchInput = initialBatchInput?.trim() ? initialBatchInput : "";
    const nextUrl = initialUrl?.trim() ? initialUrl : "";
    if (nextBatchInput) {
      setBatchInput(nextBatchInput);
      setAdvancedOpen(true);
      void runBatch(false, nextBatchInput);
      return;
    }
    if (nextUrl) {
      setUrl(nextUrl);
    }
  }, [initialBatchInput, initialSourceId, initialUrl]);

  useEffect(() => {
    onDraftStateChange?.(
      Boolean(
        url.trim() ||
          saveDir.trim() ||
          fileName.trim() ||
          expectedHashSha256.trim() ||
          batchInput.trim() ||
          selectedLocalFile,
      ),
    );
  }, [
    batchInput,
    expectedHashSha256,
    fileName,
    onDraftStateChange,
    saveDir,
    selectedLocalFile,
    url,
  ]);

  // Toggle all files
  function toggleAllFiles() {
    if (!probe) return;
    if (selectedFiles.size === probe.files.length) {
      setSelectedFiles(new Set());
    } else {
      setSelectedFiles(new Set(probe.files.map((_, i) => i)));
    }
  }

  // Toggle single file
  function toggleFile(index: number) {
    setSelectedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  }

  // Computed totals
  const selectedTotal = useMemo(() => {
    if (!probe) return 0;
    let total = 0;
    for (const idx of selectedFiles) {
      total += parseByteCount(probe.files[idx]?.size ?? "0");
    }
    return total;
  }, [probe, selectedFiles]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("newDownload.title")}</DialogTitle>
          <DialogDescription className="sr-only">
            {t("newDownload.description")}
          </DialogDescription>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col overflow-hidden" onSubmit={submit}>
          <DialogBody className="flex flex-col gap-3 py-4">
            {/* URL input */}
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.url")}
              <div className="flex gap-2">
                <Input
                  value={url}
                  onChange={(event) => {
                    setUrl(event.target.value);
                    setProbe(null);
                    setProbeUrl("");
                    setFtpDirectoryProbe(null);
                    if (selectedLocalFile?.kind === "torrent" || selectedLocalFile?.kind === "metalink") {
                      setSelectedLocalFile(null);
                    }
                  }}
                  placeholder={t("newDownload.urlPlaceholder")}
                  className="h-11 min-w-0 flex-1 md:h-8"
                  autoFocus
                  required
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-11 shrink-0 md:h-8"
                  onClick={chooseLocalFile}
                  disabled={fileLoading || submitting}
                  title={t("newDownload.chooseFile")}
                >
                  <File className="h-4 w-4" />
                  <span className="hidden sm:inline">{t("newDownload.chooseFile")}</span>
                </Button>
              </div>
            </label>

            {shouldShowManifestProtocolHint ? (
              <p className="text-[11px] leading-4 text-text-muted">
                {t("newDownload.manifestProtocolHint")}
              </p>
            ) : null}

            {canProbeFtpDirectory ? (
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8"
                  onClick={() => void runFtpDirectoryProbe()}
                  disabled={ftpDirectoryLoading || submitting}
                >
                  {ftpDirectoryLoading ? t("newDownload.probing") : t("newDownload.probeDirectory")}
                </Button>
                <span className="text-[11px] text-text-muted">{t("newDownload.ftpDirectoryHint")}</span>
              </div>
            ) : null}

            {ftpDirectoryProbe ? (
              <div className="rounded-md border border-border-subtle bg-surface-raised/50 p-3 text-xs">
                <div className="mb-2 flex items-center justify-between gap-2">
                  <span className="font-medium text-text-secondary">{t("newDownload.ftpDirectory")}</span>
                  <span className="text-text-muted">{ftpDirectoryProbe.entries.length}</span>
                </div>
                <div className="max-h-32 space-y-1 overflow-auto pr-1">
                  {ftpDirectoryProbe.entries.map((entry) => (
                    <button
                      type="button"
                      key={`${entry.name}-${entry.raw}`}
                      className="flex w-full items-center justify-between gap-3 rounded-sm px-2 py-1 text-left hover:bg-surface-hover disabled:opacity-60"
                      disabled={!entry.probableFileUrl}
                      onClick={() => {
                        if (entry.probableFileUrl) {
                          setUrl(entry.probableFileUrl);
                          setFtpDirectoryProbe(null);
                        }
                      }}
                    >
                      <span className="truncate text-text-secondary">{entry.name}</span>
                      <span className="shrink-0 text-text-muted">
                        {entry.probableFileUrl ? t("newDownload.useFileUrl") : t("newDownload.directoryEntry")}
                      </span>
                    </button>
                  ))}
                </div>
                {ftpDirectoryProbe.diagnostics.length > 0 ? (
                  <p className="mt-2 truncate text-text-muted">{ftpDirectoryProbe.diagnostics[0]}</p>
                ) : null}
              </div>
            ) : null}

            {/* Selected local file card (from file picker) */}
            {selectedLocalFile ? (
              <div className="flex items-center gap-3 rounded-md border border-border-subtle bg-surface-raised/60 px-3 py-2.5">
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-accent-primary/10 text-accent-primary">
                  {selectedLocalFile.kind === "torrent" || selectedLocalFile.kind === "metalink" ? (
                    <File className="h-5 w-5" />
                  ) : (
                    <FileText className="h-5 w-5" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-text-primary">{selectedLocalFile.name}</p>
                  <p className="text-xs text-text-muted">
                    {localFileKindLabel(selectedLocalFile.kind, t)}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={clearSelectedLocalFile}
                  className="shrink-0 rounded p-1 text-text-muted transition-colors hover:bg-surface-raised hover:text-text-secondary"
                  title={t("newDownload.removeFile")}
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            ) : null}

            {/* Save directory */}
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.saveDir")}
              <div className="flex gap-2">
                <Input
                  value={saveDir}
                  onChange={(event) => setSaveDir(event.target.value)}
                  placeholder={
                    settings?.defaultSaveDir ?? t("newDownload.saveDirPlaceholder")
                  }
                  className="h-11 min-w-0 flex-1 md:h-8"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  className="h-11 shrink-0 md:h-8 md:w-8"
                  onClick={chooseDirectory}
                  disabled={submitting}
                  title={t("newDownload.chooseDirectory")}
                >
                  <FolderOpen className="h-4 w-4" />
                </Button>
              </div>
            </label>

            {/* ---- File selection card (from probe) ---- */}
            {probe ? (
              <div className="rounded-md border border-border-subtle bg-surface-raised/40 overflow-hidden">
                <div className="flex items-center gap-2 border-b border-border-separator px-3 py-1.5 text-[11px] text-text-muted">
                  <span className="rounded bg-surface-raised px-1.5 py-0.5 font-medium text-text-secondary">
                    {probe.protocol === "bt" ? "BT" :
                     probe.protocol === "magnet" ? "Magnet" :
                     probe.protocol === "metalink" ? "Metalink" :
                     probe.protocol === "hls" ? "HLS" :
                     probe.protocol === "ftp" ? "FTP" : "HTTP"}
                  </span>
                  {probe.capabilities.supportsResume ? (
                    <span>{t("newDownload.probeResumable")}</span>
                  ) : (
                    <span>{t("newDownload.probeSingleConnection")}</span>
                  )}
                </div>
                {/* === Multi-file mode === */}
                {isMultiFile ? (
                  <>
                    {/* Header bar */}
                    <div className="flex items-center justify-between border-b border-border-subtle px-3 py-2">
                      <button
                        type="button"
                        role="checkbox"
                        aria-checked={selectedFiles.size === probe.files.length}
                        aria-label={
                          selectedFiles.size === probe.files.length
                            ? t("newDownload.deselectAll")
                            : t("newDownload.selectAll")
                        }
                        onClick={toggleAllFiles}
                        className="flex items-center gap-2 text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/70"
                      >
                        <span
                          aria-hidden="true"
                          className={`flex h-4 w-4 items-center justify-center rounded border transition-colors ${
                            selectedFiles.size === probe.files.length
                              ? "border-accent-primary bg-accent-primary text-text-on-accent"
                              : "border-border-subtle"
                          }`}
                        >
                          {selectedFiles.size === probe.files.length ? (
                            <Check className="h-3 w-3" />
                          ) : null}
                        </span>
                        {selectedFiles.size === probe.files.length
                          ? t("newDownload.deselectAll")
                          : t("newDownload.selectAll")}
                      </button>
                      <span className="text-xs text-text-muted">
                        {t("newDownload.selectedCount", {
                          count: selectedFiles.size,
                          total: probe.files.length,
                        })}
                      </span>
                    </div>

                    {/* File list */}
                    <div className="max-h-48 overflow-y-auto overscroll-contain">
                      {probe.files.map((file, idx) => (
                        <FileRow
                          key={`${file.relativePath}-${idx}`}
                          file={file}
                          index={idx}
                          checked={selectedFiles.has(idx)}
                          onToggle={() => toggleFile(idx)}
                        />
                      ))}
                    </div>

                    {/* Footer summary */}
                    <div className="flex items-center justify-between border-t border-border-subtle px-3 py-2 text-xs text-text-muted">
                      <span>{t("newDownload.totalSize")}</span>
                      <span className="font-mono text-text-primary">
                        {formatBytes(selectedTotal)}
                      </span>
                    </div>
                  </>
                ) : (
                  /* === Single-file mode === */
                  <div className="flex items-center gap-3 px-3 py-2.5">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-accent-primary/10 text-accent-primary">
                      {fileIcon(probe.fileName, "h-5 w-5")}
                    </div>
                    <div className="min-w-0 flex-1">
                      {editingName ? (
                        <Input
                          value={fileName}
                          onChange={(e) => setFileName(e.target.value)}
                          onBlur={() => setEditingName(false)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              setEditingName(false);
                            }
                          }}
                          className="h-7 text-sm"
                          autoFocus
                        />
                      ) : (
                        <button
                          type="button"
                          onClick={() => setEditingName(true)}
                          className="group flex w-full items-center gap-1.5 text-left"
                          title={t("newDownload.editFileName")}
                        >
                          <span className="truncate text-sm text-text-primary">
                            {fileName || probe.fileName}
                          </span>
                          <Pencil className="h-3 w-3 shrink-0 text-text-muted opacity-0 transition-opacity group-hover:opacity-100" />
                        </button>
                      )}
                      <div className="mt-0.5 flex items-center gap-3 text-xs text-text-muted">
                        <span>{formatBytes(parseByteCount(probe.totalSize))}</span>
                        <span>
                          {probe.capabilities.supportsResume
                            ? t("newDownload.resumeSupported")
                            : t("newDownload.resumeUnavailable")}
                        </span>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            ) : null}

            {/* Auto-detecting indicator */}
            {!probe && probing ? (
              <p className="text-xs text-text-muted">
                {t("newDownload.autoDetecting")}
              </p>
            ) : null}

            {/* Advanced options toggle */}
            <button
              type="button"
              onClick={() => setAdvancedOpen((v) => !v)}
              className="flex items-center gap-1.5 self-start text-xs text-text-secondary transition-colors hover:text-text-primary"
            >
              <ChevronDown
                className={`h-3.5 w-3.5 transition-transform duration-200 ${advancedOpen ? "" : "-rotate-90"}`}
              />
              {t("newDownload.advancedOptions")}
            </button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-8 self-start px-2 text-xs"
              onClick={openBatchImport}
            >
              <ListPlus className="h-4 w-4" />
              {t("newDownload.batchImport")}
            </Button>

            {/* Advanced section */}
            {advancedOpen ? (
              <div className="flex flex-col gap-3 rounded-md border border-border-subtle bg-surface-root/30 p-3">
                {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe ? (
                  <label className="flex flex-col gap-1 text-xs text-text-muted">
                    {t("newDownload.sha256")}
                    <Input
                      value={expectedHashSha256}
                      onChange={(event) => setExpectedHashSha256(event.target.value)}
                      placeholder={t("newDownload.sha256Placeholder")}
                      className="h-8 font-mono"
                    />
                  </label>
                ) : null}

                {/* Batch import */}
                <div className="flex flex-col gap-2 border-t border-border-subtle pt-3">
                  <span className="text-xs text-text-muted">
                    {t("newDownload.batchUrls")}
                  </span>
                  <textarea
                    ref={batchInputRef}
                    value={batchInput}
                    onChange={(event) => setBatchInput(event.target.value)}
                    placeholder={t("newDownload.batchUrlsPlaceholder")}
                    className="min-h-20 resize-y rounded-md border border-border-subtle bg-surface-base px-3 py-2 font-mono text-xs text-text-primary outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                  />
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8"
                      onClick={() => void runBatch(false)}
                      disabled={submitting || !batchInput.trim()}
                    >
                      {t("newDownload.previewBatch")}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8"
                      onClick={() => void runBatch(true)}
                      disabled={submitting || !batchInput.trim()}
                    >
                      {t("newDownload.createBatch")}
                    </Button>
                  </div>
                  {batchResult ? (
                    <BatchResultSummary result={batchResult} />
                  ) : null}
                </div>
              </div>
            ) : null}

            {/* Submit status */}
            {submitStatus ? (
              <p role="status" className="rounded-md border border-border-accent bg-accent-primary/10 px-3 py-2 text-xs text-accent-primary">
                {submitStatus}
              </p>
            ) : null}

            {/* Error */}
            {error ? (
              <div role="alert" className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
                <p>{error}</p>
                {duplicateOverrideAvailable ? (
                  <div className="mt-2 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <p className="text-text-secondary">
                      {t("newDownload.duplicateHint")}
                    </p>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8 shrink-0"
                      disabled={submitting}
                      onClick={() => void submitDuplicateOverride()}
                    >
                      {t("newDownload.createDuplicate")}
                    </Button>
                  </div>
                ) : (
                  <p className="mt-1 text-text-secondary">
                    {t("newDownload.probeFailedHint")}
                  </p>
                )}
              </div>
            ) : null}
          </DialogBody>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              className="w-full sm:w-auto"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("newDownload.cancel")}
            </Button>
            <Button
              type="submit"
              className="w-full sm:w-auto"
              disabled={submitting || fileSelectionRequired || !url.trim()}
            >
              {submitting ? (
                <>
                  <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                  {t("newDownload.starting")}
                </>
              ) : t("newDownload.start")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/* ------------------------------------------------------------------ */
/*  Sub-components                                                      */
/* ------------------------------------------------------------------ */

function FileRow({
  file,
  index,
  checked,
  onToggle,
}: {
  file: ProbedFile;
  index: number;
  checked: boolean;
  onToggle: () => void;
}) {
  const size = parseByteCount(file.size);
  // Show just the filename if the relativePath is a single segment
  const displayName = file.relativePath.split("/").pop() ?? file.relativePath;
  const hasPath = file.relativePath.includes("/");

  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={displayName}
      onClick={onToggle}
      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-surface-raised/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary/70 ${
        index > 0 ? "border-t border-border-subtle/50" : ""
      }`}
    >
      <span
        aria-hidden="true"
        className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors ${
          checked
            ? "border-accent-primary bg-accent-primary text-text-on-accent"
            : "border-border-subtle"
        }`}
      >
        {checked ? <Check className="h-3 w-3" /> : null}
      </span>
      <span className="shrink-0 text-text-muted">{fileIcon(displayName)}</span>
      <div className="min-w-0 flex-1">
        <p className={`truncate text-sm ${checked ? "text-text-primary" : "text-text-muted"}`}>
          {displayName}
        </p>
        {hasPath ? (
          <p className="truncate text-[10px] text-text-muted">
            {file.relativePath.slice(0, file.relativePath.length - displayName.length - 1)}
          </p>
        ) : null}
      </div>
      <span className={`shrink-0 font-mono text-xs tabular-nums ${checked ? "text-text-secondary" : "text-text-muted"}`}>
        {formatBytes(size)}
      </span>
    </button>
  );
}

function BatchResultSummary({ result }: { result: BatchImportResult }) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-2 text-xs">
      <p className="text-text-secondary">
        {t("newDownload.batchSummary", {
          total: result.items.length,
          created: result.createdCount,
          failed: result.failedCount,
          duplicate: result.duplicateCount,
        })}
      </p>
      {result.items.slice(0, 5).map((item) => (
        <div
          key={`${item.inputUrl}-${item.normalizedUrl ?? "invalid"}`}
          className="grid gap-1 rounded-md border border-border-divider bg-surface-raised/50 px-2 py-1"
        >
          <span className="truncate font-mono text-text-primary">
            {item.fileName ?? sanitizeUrlForDisplay(item.normalizedUrl ?? item.inputUrl)}
          </span>
          <span className={item.valid ? "text-text-muted" : "text-status-danger"}>
            {item.errorMessage ?? (item.task ? t("newDownload.batchCreated") : t("newDownload.batchReady"))}
          </span>
        </div>
      ))}
    </div>
  );
}
