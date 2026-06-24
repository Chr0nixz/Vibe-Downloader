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
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type {
  BatchImportResult,
  FtpDirectoryProbe,
  ProbedFile,
  ProbeTaskPayload,
  SftpDirectoryProbe,
  WebDavDirectoryProbe,
} from "@/generated/bindings";
import { localizedErrorMessage, parseAppError } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import {
  createTask,
  importUrls,
  openDirectoryPicker,
  openFilePicker,
  probeFtpDirectory,
  probeSftpDirectory,
  probeTask,
  probeWebdavDirectory,
} from "@/lib/tauri";

const log = createLogger("new-download");

import { getLocalFileKind, pathToFileUrl, readFileAsText } from "@/lib/local-file";
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import type { Task } from "@/types/task";
import { normalizeTask, parseByteCount } from "@/types/task";

/* ------------------------------------------------------------------ */
/*  Probe error classification                                       */
/* ------------------------------------------------------------------ */

function probeErrorHintKey(rawError: unknown, message: string): string {
  // Prefer structured error codes from AppErrorPayload when available.
  const appError = parseAppError(rawError);
  if (appError) {
    switch (appError.code) {
      case "dns_failure":
        return "newDownload.probeErrorDns";
      case "http_denied":
      case "sftp_auth_failed":
      case "sftp_credentials_required":
        return "newDownload.probeErrorDenied";
      case "http_not_found":
      case "sftp_directory_not_file":
        return "newDownload.probeErrorNotFound";
      case "server_rate_limited":
        return "newDownload.probeErrorRateLimited";
      case "timeout":
        return "newDownload.probeErrorTimeout";
      case "connection_refused":
      case "network_unreachable":
      case "sftp_connect_failed":
      case "sftp_proxy_connect_failed":
      case "ftp_connect_failed":
        return "newDownload.probeErrorConnection";
      case "tls_error":
        return "newDownload.probeErrorTls";
      case "hls_ffmpeg_missing":
      case "dash_ffmpeg_missing":
        return "newDownload.probeErrorFfmpeg";
      case "hls_invalid_playlist":
      case "hls_unsupported_encryption":
      case "dash_invalid_manifest":
      case "dash_live_unsupported":
      case "metalink_invalid_manifest":
      case "metalink_no_resources":
        return "newDownload.probeErrorManifest";
      default:
        break;
    }
  }

  // Fallback: string-match legacy plain-text errors.
  const lower = message.toLowerCase();
  if (
    lower.includes("dns") ||
    lower.includes("resolve") ||
    lower.includes("name or service not known") ||
    lower.includes("getaddrinfo")
  ) {
    return "newDownload.probeErrorDns";
  }
  if (
    /\b403\b/.test(message) ||
    lower.includes("forbidden") ||
    lower.includes("denied") ||
    lower.includes("unauthorized")
  ) {
    return "newDownload.probeErrorDenied";
  }
  if (/\b404\b/.test(message) || lower.includes("not found")) {
    return "newDownload.probeErrorNotFound";
  }
  if (/\b429\b/.test(message) || lower.includes("rate") || lower.includes("too many")) {
    return "newDownload.probeErrorRateLimited";
  }
  if (lower.includes("timeout") || lower.includes("timed out") || lower.includes("deadline")) {
    return "newDownload.probeErrorTimeout";
  }
  if (
    lower.includes("connection") ||
    lower.includes("connect") ||
    lower.includes("refused") ||
    lower.includes("network") ||
    lower.includes("unreachable")
  ) {
    return "newDownload.probeErrorConnection";
  }
  return "newDownload.probeFailedHint";
}

/* ------------------------------------------------------------------ */
/*  Local file picker types                                            */
/* ------------------------------------------------------------------ */

interface SelectedLocalFile {
  path: string;
  name: string;
  kind: "torrent" | "metalink" | "dash" | "text";
}

type RemoteDirectoryProbe = FtpDirectoryProbe | SftpDirectoryProbe | WebDavDirectoryProbe;
type RemoteDirectoryEntry = RemoteDirectoryProbe["entries"][number];

function remoteDirectoryProtocolLabel(input: string): string {
  const lower = input.trim().toLowerCase();
  if (lower.startsWith("webdavs://")) return "WebDAVS";
  if (lower.startsWith("webdav://")) return "WebDAV";
  if (lower.startsWith("sftp://")) return "SFTP";
  if (lower.startsWith("ftps://")) return "FTPS";
  return "FTP";
}

function remoteDirectoryEntryKey(entry: RemoteDirectoryEntry): string {
  return "raw" in entry ? `${entry.name}-${entry.raw}` : `${entry.name}-${entry.href}`;
}

function localFileKindLabel(kind: SelectedLocalFile["kind"], t: (key: string) => string): string {
  if (kind === "torrent") return t("newDownload.fileKindTorrent");
  if (kind === "metalink") return t("newDownload.fileKindMetalink");
  if (kind === "dash") return t("newDownload.fileKindDash");
  return t("newDownload.fileKindText");
}

/* ------------------------------------------------------------------ */
/*  File icon helper                                                    */
/* ------------------------------------------------------------------ */

function fileIcon(name: string, className = "h-4 w-4") {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst"].includes(ext)) return <FileArchive className={className} />;
  if (["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "ts"].includes(ext))
    return <FileVideo className={className} />;
  if (["mp3", "flac", "wav", "aac", "ogg", "m4a", "opus"].includes(ext)) return <FileAudio className={className} />;
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
  const [rawError, setRawError] = useState<unknown>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyData, setPrivateKeyData] = useState("");
  const [privateKeyPassphrase, setPrivateKeyPassphrase] = useState("");

  function setFormError(err: unknown) {
    setError(localizedErrorMessage(err, t));
    setRawError(err);
  }
  function clearFormError() {
    setError(null);
    setRawError(null);
  }
  const [remoteDirectoryProbe, setRemoteDirectoryProbe] = useState<RemoteDirectoryProbe | null>(null);
  const [remoteDirectoryLoading, setRemoteDirectoryLoading] = useState(false);
  const [duplicateOverrideAvailable, setDuplicateOverrideAvailable] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<string | null>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [selectedLocalFile, setSelectedLocalFile] = useState<SelectedLocalFile | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [editingName, setEditingName] = useState(false);

  // Multi-file selection: set of selected indices from probe.files
  const [selectedFiles, setSelectedFiles] = useState<Set<number>>(new Set());
  const [selectedHlsVariantUri, setSelectedHlsVariantUri] = useState<string | null>(null);

  const probeRequestId = useRef(0);
  const batchInputRef = useRef<HTMLTextAreaElement | null>(null);
  const appliedInitialSourceId = useRef<string | undefined>(undefined);
  const isTorrentProbe = probe?.protocol === "bt" || probe?.protocol === "magnet";
  const isMetalinkProbe = probe?.protocol === "metalink";
  const isHlsProbe = probe?.protocol === "hls";
  const isDashProbe = probe?.protocol === "dash";
  const isSftpUrl = /^sftp:\/\//i.test(url.trim());
  const isSelectableMultiFileProbe = isTorrentProbe || isMetalinkProbe;
  const isMultiFile = probe != null && probe.files.length > 1;
  const shouldShowManifestProtocolHint =
    isTorrentProbe ||
    isMetalinkProbe ||
    isHlsProbe ||
    isDashProbe ||
    selectedLocalFile?.kind === "torrent" ||
    selectedLocalFile?.kind === "metalink" ||
    selectedLocalFile?.kind === "dash" ||
    /\.(torrent|meta4|metalink|m3u8|mpd)(?:[?#].*)?$/i.test(url.trim());
  const fileSelectionRequired = isSelectableMultiFileProbe && isMultiFile && selectedFiles.size === 0;
  const canProbeRemoteDirectory = /^(ftp|ftps|sftp|webdav|webdavs):\/\//i.test(url.trim()) && url.trim().endsWith("/");

  // Initialize selectedFiles when probe changes
  useEffect(() => {
    if (probe && probe.files.length > 1) {
      setSelectedFiles(new Set(probe.files.map((_, i) => i)));
    } else {
      setSelectedFiles(new Set());
    }
    if (probe && probe.hlsVariants.length > 1) {
      const autoSelected = probe.hlsVariants.find((v) => v.selected);
      setSelectedHlsVariantUri(autoSelected?.uri ?? probe.hlsVariants[0].uri);
    } else {
      setSelectedHlsVariantUri(null);
    }
  }, [probe]);

  async function detect(nextUrl = url.trim(), automatic = false) {
    if (!nextUrl) return;
    const requestId = ++probeRequestId.current;
    setProbing(true);
    setDuplicateOverrideAvailable(false);
    if (!automatic) clearFormError();
    setProbe(null);
    setProbeUrl("");
    setRemoteDirectoryProbe(null);
    try {
      const nextProbe = await probeTask({
        url: nextUrl,
        username: username.trim() || null,
        password: password || null,
        privateKeyData: privateKeyData || null,
        privateKeyPassphrase: privateKeyPassphrase || null,
      });
      if (requestId !== probeRequestId.current) return;
      setProbe(nextProbe);
      setProbeUrl(nextUrl);
      if (!fileName.trim()) {
        setFileName(nextProbe.fileName);
      }
      clearFormError();
    } catch (err) {
      if (requestId !== probeRequestId.current) return;
      log.warn("probe failed", err);
      setFormError(err);
    } finally {
      if (requestId === probeRequestId.current) setProbing(false);
    }
  }

  async function runRemoteDirectoryProbe() {
    const nextUrl = url.trim();
    setRemoteDirectoryLoading(true);
    setError(null);
    try {
      setRemoteDirectoryProbe(
        /^webdavs?:\/\//i.test(nextUrl)
          ? await probeWebdavDirectory(nextUrl)
          : /^sftp:\/\//i.test(nextUrl)
            ? await probeSftpDirectory(nextUrl)
            : await probeFtpDirectory(nextUrl),
      );
    } catch (err) {
      setError(localizedErrorMessage(err, t));
    } finally {
      setRemoteDirectoryLoading(false);
    }
  }

  useEffect(() => {
    const nextUrl = url.trim();
    setSubmitStatus(null);
    setDuplicateOverrideAvailable(false);
    if (!nextUrl) {
      setProbe(null);
      setProbeUrl("");
      setRemoteDirectoryProbe(null);
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
    const currentIsTorrentProbe = currentProbe?.protocol === "bt" || currentProbe?.protocol === "magnet";
    const currentIsMetalinkProbe = currentProbe?.protocol === "metalink";
    const currentIsHlsProbe = currentProbe?.protocol === "hls";
    const currentIsDashProbe = currentProbe?.protocol === "dash";
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
    setSubmitStatus(currentProbe ? t("newDownload.usingProbe") : t("newDownload.revalidating"));
    try {
      const task = await createTask({
        url: currentUrl,
        saveDir: saveDir.trim() || null,
        fileName: fileName.trim() || null,
        expectedHashSha256:
          currentIsTorrentProbe || currentIsMetalinkProbe || currentIsHlsProbe || currentIsDashProbe
            ? null
            : expectedHashSha256.trim() || null,
        taskSpeedLimitBps: null,
        priority: null,
        categoryKey: null,
        probeSnapshot: currentProbe,
        selectedFilePaths,
        allowDuplicate,
        username: username.trim() || null,
        password: password || null,
        privateKeyData: privateKeyData || null,
        privateKeyPassphrase: privateKeyPassphrase || null,
        selectedHlsVariantUri: currentIsHlsProbe && selectedHlsVariantUri ? selectedHlsVariantUri : null,
      });
      onCreated(task);
      resetForm();
      onOpenChange(false);
    } catch (err) {
      log.error("create task failed", err);
      const appError = parseAppError(err);
      setDuplicateOverrideAvailable(appError?.code === "duplicate_task");
      setError(localizedErrorMessage(err, t));
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
    setRemoteDirectoryProbe(null);
    setDuplicateOverrideAvailable(false);
    setSubmitStatus(null);
    setAdvancedOpen(false);
    setSelectedLocalFile(null);
    setSelectedFiles(new Set());
    setEditingName(false);
    setUsername("");
    setPassword("");
    setPrivateKeyData("");
    setPrivateKeyPassphrase("");
    setSelectedHlsVariantUri(null);
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setSaveDir(selected);
  }

  async function chooseLocalFile() {
    setFileLoading(true);
    try {
      const picked = await openFilePicker([
        { name: "Manifest / Text", extensions: ["torrent", "meta4", "metalink", "mpd", "txt"] },
      ]);
      if (!picked) return;

      const kind = getLocalFileKind(picked.name);
      const file: SelectedLocalFile = { path: picked.path, name: picked.name, kind };
      setSelectedLocalFile(file);

      if (kind === "torrent" || kind === "metalink" || kind === "dash") {
        const fileUrl = pathToFileUrl(picked.path);
        setUrl(fileUrl);
        setProbe(null);
        setProbeUrl("");
        setRemoteDirectoryProbe(null);
      } else {
        try {
          const text = await readFileAsText(picked.path);
          setBatchInput(text);
          setAdvancedOpen(true);
        } catch (err) {
          log.warn("failed to read text file", err);
          setError(localizedErrorMessage(err, t));
        }
      }
    } catch (err) {
      log.error("file picker failed", err);
      setError(localizedErrorMessage(err, t));
    } finally {
      setFileLoading(false);
    }
  }

  async function chooseSshKeyFile() {
    try {
      const picked = await openFilePicker([
        { name: "SSH Private Key", extensions: ["pem", "key", "id_rsa", "id_ed25519", "id_ecdsa", ""] },
      ]);
      if (!picked) return;
      const content = await readFileAsText(picked.path);
      setPrivateKeyData(content);
    } catch (err) {
      log.error("SSH key file picker failed", err);
      setError(localizedErrorMessage(err, t));
    }
  }

  function clearSelectedLocalFile() {
    setSelectedLocalFile(null);
    if (
      selectedLocalFile?.kind === "torrent" ||
      selectedLocalFile?.kind === "metalink" ||
      selectedLocalFile?.kind === "dash"
    ) {
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
      setError(localizedErrorMessage(err, t));
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
  }, [batchInput, expectedHashSha256, fileName, onDraftStateChange, saveDir, selectedLocalFile, url]);

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
          <DialogDescription className="sr-only">{t("newDownload.description")}</DialogDescription>
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
                    setRemoteDirectoryProbe(null);
                    if (
                      selectedLocalFile?.kind === "torrent" ||
                      selectedLocalFile?.kind === "metalink" ||
                      selectedLocalFile?.kind === "dash"
                    ) {
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
              <p className="text-[11px] leading-4 text-text-muted">{t("newDownload.manifestProtocolHint")}</p>
            ) : null}

            {isDashProbe ? (
              <div
                role="note"
                className="rounded-md border border-border-warning bg-status-warning/10 px-3 py-2 text-[11px] leading-4 text-status-warning"
              >
                <p className="font-medium">{t("newDownload.dashLimitationsTitle")}</p>
                <p>{t("newDownload.dashLimitationsDescription")}</p>
              </div>
            ) : null}

            {canProbeRemoteDirectory ? (
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8"
                  onClick={() => void runRemoteDirectoryProbe()}
                  disabled={remoteDirectoryLoading || submitting}
                >
                  {remoteDirectoryLoading ? t("newDownload.probing") : t("newDownload.probeDirectory")}
                </Button>
                <span className="text-[11px] text-text-muted">{t("newDownload.remoteDirectoryHint")}</span>
              </div>
            ) : null}

            {remoteDirectoryProbe ? (
              <div className="rounded-md border border-border-subtle bg-surface-raised/50 p-3 text-xs">
                <div className="mb-2 flex items-center justify-between gap-2">
                  <span className="font-medium text-text-secondary">
                    {t("newDownload.remoteDirectory", {
                      protocol: remoteDirectoryProtocolLabel(
                        remoteDirectoryProbe.directoryUrl || remoteDirectoryProbe.inputUrl,
                      ),
                    })}
                  </span>
                  <span className="text-text-muted">{remoteDirectoryProbe.entries.length}</span>
                </div>
                <div className="max-h-32 space-y-1 overflow-auto pr-1">
                  {remoteDirectoryProbe.entries.map((entry) => (
                    <button
                      type="button"
                      key={remoteDirectoryEntryKey(entry)}
                      className="flex w-full items-center justify-between gap-3 rounded-sm px-2 py-1 text-left hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary disabled:opacity-60"
                      disabled={!entry.probableFileUrl}
                      onClick={() => {
                        if (entry.probableFileUrl) {
                          setUrl(entry.probableFileUrl);
                          setRemoteDirectoryProbe(null);
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
                {remoteDirectoryProbe.diagnostics.length > 0 ? (
                  <p className="mt-2 truncate text-text-muted">{remoteDirectoryProbe.diagnostics[0]}</p>
                ) : null}
              </div>
            ) : null}

            {/* Selected local file card (from file picker) */}
            {selectedLocalFile ? (
              <div className="flex items-center gap-3 rounded-md border border-border-subtle bg-surface-raised/60 px-3 py-2.5">
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-accent-primary/10 text-accent-primary">
                  {selectedLocalFile.kind === "torrent" || selectedLocalFile.kind === "metalink" ? (
                    <File className="h-5 w-5" />
                  ) : selectedLocalFile.kind === "dash" ? (
                    <FileVideo className="h-5 w-5" />
                  ) : (
                    <FileText className="h-5 w-5" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-text-primary">{selectedLocalFile.name}</p>
                  <p className="text-xs text-text-muted">{localFileKindLabel(selectedLocalFile.kind, t)}</p>
                </div>
                <button
                  type="button"
                  onClick={clearSelectedLocalFile}
                  className="shrink-0 rounded p-1.5 min-h-8 min-w-8 text-text-muted transition-colors hover:bg-surface-raised hover:text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
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
                  placeholder={settings?.defaultSaveDir ?? t("newDownload.saveDirPlaceholder")}
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
                    {probe.protocol === "bt"
                      ? "BT"
                      : probe.protocol === "magnet"
                        ? "Magnet"
                        : probe.protocol === "metalink"
                          ? "Metalink"
                          : probe.protocol === "hls"
                            ? "HLS"
                            : probe.protocol === "dash"
                              ? "DASH"
                              : probe.protocol === "ftp"
                                ? "FTP"
                                : probe.protocol === "ftps"
                                  ? "FTPS"
                                  : probe.protocol === "sftp"
                                    ? "SFTP"
                                    : probe.protocol === "webdav"
                                      ? "WebDAV"
                                      : probe.protocol === "webdavs"
                                        ? "WebDAVS"
                                        : "HTTP"}
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
                        className="flex items-center gap-2 text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                      >
                        <span
                          aria-hidden="true"
                          className={`flex h-4 w-4 items-center justify-center rounded border transition-colors ${
                            selectedFiles.size === probe.files.length
                              ? "border-accent-primary bg-accent-primary text-text-on-accent"
                              : "border-border-subtle"
                          }`}
                        >
                          {selectedFiles.size === probe.files.length ? <Check className="h-3 w-3" /> : null}
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
                      <span className="font-mono text-text-primary">{formatBytes(selectedTotal)}</span>
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
                          aria-label={t("newDownload.fileName")}
                          className="h-7 text-sm"
                          autoFocus
                        />
                      ) : (
                        <button
                          type="button"
                          onClick={() => setEditingName(true)}
                          className="group flex w-full items-center gap-1.5 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                          title={t("newDownload.editFileName")}
                        >
                          <span className="truncate text-sm text-text-primary">{fileName || probe.fileName}</span>
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
            {!probe && probing ? <p className="text-xs text-text-muted">{t("newDownload.autoDetecting")}</p> : null}

            {/* HLS quality picker */}
            {isHlsProbe && probe && probe.hlsVariants.length > 1 ? (
              <label className="flex flex-col gap-1 text-xs text-text-muted">
                {t("newDownload.hlsQuality")}
                <Select value={selectedHlsVariantUri ?? ""} onValueChange={(value) => setSelectedHlsVariantUri(value)}>
                  <SelectTrigger className="h-8 w-full text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {probe.hlsVariants.map((variant) => (
                      <SelectItem key={variant.uri} value={variant.uri}>
                        {variant.resolution ?? t("newDownload.hlsAudioOnly")}
                        {" · "}
                        {Math.round(Number(variant.bandwidth) / 1000)} kbps
                        {variant.codecs ? ` · ${variant.codecs}` : ""}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </label>
            ) : null}

            {/* Advanced options toggle */}
            <button
              type="button"
              onClick={() => setAdvancedOpen((v) => !v)}
              className="flex items-center gap-1.5 self-start text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
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
                {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe && !isDashProbe ? (
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

                {/* Authentication */}
                {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe && !isDashProbe ? (
                  <div className="flex flex-col gap-2 border-t border-border-subtle pt-3">
                    <span className="text-xs font-medium text-text-secondary">{t("newDownload.authentication")}</span>
                    <div className="grid grid-cols-2 gap-2">
                      <label className="flex flex-col gap-1 text-xs text-text-muted">
                        {t("newDownload.authUsername")}
                        <Input
                          value={username}
                          onChange={(event) => setUsername(event.target.value)}
                          placeholder={t("newDownload.authUsernamePlaceholder")}
                          className="h-8"
                          autoComplete="username"
                        />
                      </label>
                      <label className="flex flex-col gap-1 text-xs text-text-muted">
                        {t("newDownload.authPassword")}
                        <Input
                          type="password"
                          value={password}
                          onChange={(event) => setPassword(event.target.value)}
                          placeholder={t("newDownload.authPasswordPlaceholder")}
                          className="h-8"
                          autoComplete="current-password"
                        />
                      </label>
                    </div>
                    {isSftpUrl ? (
                      <div className="flex flex-col gap-2">
                        <span className="text-xs font-medium text-text-secondary">{t("newDownload.sshKeyAuth")}</span>
                        <div className="flex items-center gap-2">
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={chooseSshKeyFile}
                            className="shrink-0"
                          >
                            <FolderOpen className="mr-1 size-3.5" />
                            {t("newDownload.sshKeyBrowse")}
                          </Button>
                          {privateKeyData ? (
                            <span className="truncate text-xs text-text-secondary">
                              {t("newDownload.sshKeyLoaded")}
                            </span>
                          ) : null}
                          {privateKeyData ? (
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              onClick={() => setPrivateKeyData("")}
                              className="shrink-0"
                            >
                              <X className="size-3.5" />
                            </Button>
                          ) : null}
                        </div>
                        <label className="flex flex-col gap-1 text-xs text-text-muted">
                          {t("newDownload.sshKeyPassphrase")}
                          <Input
                            type="password"
                            value={privateKeyPassphrase}
                            onChange={(event) => setPrivateKeyPassphrase(event.target.value)}
                            placeholder={t("newDownload.sshKeyPassphrasePlaceholder")}
                            className="h-8"
                            autoComplete="current-password"
                            disabled={!privateKeyData}
                          />
                        </label>
                      </div>
                    ) : null}
                  </div>
                ) : null}

                {/* Batch import */}
                <div className="flex flex-col gap-2 border-t border-border-subtle pt-3">
                  <label htmlFor="batch-urls-input" className="text-xs text-text-muted">
                    {t("newDownload.batchUrls")}
                  </label>
                  <textarea
                    id="batch-urls-input"
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
                  {batchResult ? <BatchResultSummary result={batchResult} /> : null}
                </div>
              </div>
            ) : null}

            {/* Submit status */}
            {submitStatus ? (
              <p
                role="status"
                className="rounded-md border border-border-accent bg-accent-primary/10 px-3 py-2 text-xs text-accent-primary"
              >
                {submitStatus}
              </p>
            ) : null}

            {/* Error */}
            {error ? (
              <div
                role="alert"
                className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
              >
                <p>{error}</p>
                {duplicateOverrideAvailable ? (
                  <div className="mt-2 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <p className="text-text-secondary">{t("newDownload.duplicateHint")}</p>
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
                  <p className="mt-1 text-text-secondary">{t(probeErrorHintKey(rawError, error))}</p>
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
              ) : (
                t("newDownload.start")
              )}
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
      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-surface-raised/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary ${
        index > 0 ? "border-t border-border-subtle/50" : ""
      }`}
    >
      <span
        aria-hidden="true"
        className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors ${
          checked ? "border-accent-primary bg-accent-primary text-text-on-accent" : "border-border-subtle"
        }`}
      >
        {checked ? <Check className="h-3 w-3" /> : null}
      </span>
      <span className="shrink-0 text-text-muted">{fileIcon(displayName)}</span>
      <div className="min-w-0 flex-1">
        <p className={`truncate text-sm ${checked ? "text-text-primary" : "text-text-muted"}`}>{displayName}</p>
        {hasPath ? (
          <p className="truncate text-[10px] text-text-muted">
            {file.relativePath.slice(0, file.relativePath.length - displayName.length - 1)}
          </p>
        ) : null}
      </div>
      <span
        className={`shrink-0 font-mono text-xs tabular-nums ${checked ? "text-text-secondary" : "text-text-muted"}`}
      >
        {formatBytes(size)}
      </span>
    </button>
  );
}

function BatchResultSummary({ result }: { result: BatchImportResult }) {
  const { t } = useTranslation();
  return (
    <div role="status" aria-live="polite" className="grid gap-2 text-xs">
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
