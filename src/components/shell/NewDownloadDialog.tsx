import {
  ChevronDown,
  File,
  FileArchive,
  FileAudio,
  FileImage,
  FileText,
  FileVideo,
  FolderOpen,
  Pencil,
  X,
} from "lucide-react";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type {
  BatchImportResult,
  ChecksumAlgorithm,
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
  onProbePhase,
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
/*  Probe phase tracking                                             */
/* ------------------------------------------------------------------ */

type ProbePhase =
  | { kind: "idle" }
  | { kind: "classifying" }
  | { kind: "connecting" }
  | { kind: "fetching_manifest" }
  | { kind: "parsing_manifest" }
  | { kind: "listing_directory" }
  | { kind: "checking_ffmpeg" }
  | { kind: "querying_metadata" }
  | { kind: "verifying_host_key" }
  | { kind: "parsing_magnet" }
  | { kind: "fetching_torrent" }
  | { kind: "inspecting_metadata" }
  | { kind: "done" }
  | { kind: "failed"; hintKey?: string };

/**
 * UX-6: Initial pre-probe hint based on URL scheme. Shown only until the
 * first real `probe-phase` event arrives from the Rust engine, which then
 * takes over and drives the indicator with accurate stages.
 */
function inferProbePhaseFromUrl(url: string): ProbePhase {
  const trimmed = url.trim();
  if (!trimmed) return { kind: "classifying" };
  if (/\.m3u8(\?|#|$)/i.test(trimmed)) {
    return { kind: "checking_ffmpeg" };
  }
  if (/\.mpd(\?|#|$)/i.test(trimmed)) {
    return { kind: "checking_ffmpeg" };
  }
  if (/\.metalink(\?|#|$)/i.test(trimmed) || /^metalink:\/\//i.test(trimmed)) {
    return { kind: "fetching_manifest" };
  }
  if (/^ftps?:\/\//i.test(trimmed)) {
    return { kind: "connecting" };
  }
  if (/^sftp:\/\//i.test(trimmed)) {
    return { kind: "connecting" };
  }
  if (/^webdavs?:\/\//i.test(trimmed)) {
    return { kind: "connecting" };
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return { kind: "connecting" };
  }
  return { kind: "classifying" };
}

function probePhaseMessageKey(phase: ProbePhase): string | null {
  switch (phase.kind) {
    case "classifying":
      return "newDownload.probePhaseClassifying";
    case "connecting":
      return "newDownload.probePhaseConnecting";
    case "fetching_manifest":
      return "newDownload.probePhaseFetchingManifest";
    case "parsing_manifest":
      return "newDownload.probePhaseParsingManifest";
    case "listing_directory":
      return "newDownload.probePhaseListingDirectory";
    case "checking_ffmpeg":
      return "newDownload.probePhaseCheckingFfmpeg";
    case "querying_metadata":
      return "newDownload.probePhaseQueryingMetadata";
    case "verifying_host_key":
      return "newDownload.probePhaseVerifyingHostKey";
    case "parsing_magnet":
      return "newDownload.probePhaseParsingMagnet";
    case "fetching_torrent":
      return "newDownload.probePhaseFetchingTorrent";
    case "inspecting_metadata":
      return "newDownload.probePhaseInspectingMetadata";
    default:
      return null;
  }
}

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

/** Short badge label for a detected download protocol. */
function protocolBadgeLabel(protocol: string): string {
  switch (protocol) {
    case "bt":
      return "BT";
    case "magnet":
      return "Magnet";
    case "metalink":
      return "Metalink";
    case "hls":
      return "HLS";
    case "dash":
      return "DASH";
    case "ftp":
      return "FTP";
    case "ftps":
      return "FTPS";
    case "sftp":
      return "SFTP";
    case "webdav":
      return "WebDAV";
    case "webdavs":
      return "WebDAVS";
    default:
      return "HTTP";
  }
}

/** i18n key for the plain-language tooltip explaining a protocol badge. */
function protocolHintKey(protocol: string): string {
  return `newDownload.protocolHint.${protocol}`;
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
  // F-5: Multi-algorithm manual hash. `expectedHashAlgorithm` selects the digest
  // algorithm; `expectedHash` carries the hex digest. When algorithm is sha256
  // and the dialog submits, both the new fields and the legacy `expectedHashSha256`
  // field are populated so older backends still accept the payload.
  const [expectedHash, setExpectedHash] = useState("");
  const [expectedHashAlgorithm, setExpectedHashAlgorithm] = useState<ChecksumAlgorithm>("sha256");
  const [submitting, setSubmitting] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probePhase, setProbePhase] = useState<ProbePhase>({ kind: "idle" });
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
  // Top-level mode toggle: single URL vs batch URLs. The batch textarea used
  // to live inside the Advanced section, which buried the most common batch
  // entry point behind a disclosure. Promoting it to a top-level segmented
  // control makes the two task-creation paths equally reachable.
  const [mode, setMode] = useState<"single" | "batch">("single");
  // Auth disclosure: most URLs are public, so credentials are hidden by
  // default and only shown when the user explicitly opts in. The underlying
  // state is preserved across toggles so a user can hide/show without losing
  // their input; submit/detect pass null when this is false.
  const [useCredentials, setUseCredentials] = useState(false);

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
  const [selectedHlsAudioTrackUris, setSelectedHlsAudioTrackUris] = useState<string[]>([]);
  const [selectedHlsSubtitleTrackUris, setSelectedHlsSubtitleTrackUris] = useState<string[]>([]);

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
    // F-6: auto-select DEFAULT audio track, and all DEFAULT subtitle tracks.
    const defaultAudio = probe?.hlsAudioTracks.filter((t) => t.default && t.uri) ?? [];
    setSelectedHlsAudioTrackUris(defaultAudio.map((t) => t.uri as string));
    const defaultSubs = probe?.hlsSubtitleTracks.filter((t) => t.default && t.uri) ?? [];
    setSelectedHlsSubtitleTrackUris(defaultSubs.map((t) => t.uri as string));
  }, [probe]);

  async function detect(nextUrl = url.trim(), automatic = false) {
    if (!nextUrl) return;
    const requestId = ++probeRequestId.current;
    setProbing(true);
    setProbePhase(inferProbePhaseFromUrl(nextUrl));
    setDuplicateOverrideAvailable(false);
    if (!automatic) clearFormError();
    setProbe(null);
    setProbeUrl("");
    setRemoteDirectoryProbe(null);
    try {
      const nextProbe = await probeTask({
        url: nextUrl,
        username: useCredentials ? username.trim() || null : null,
        password: useCredentials ? password || null : null,
        privateKeyData: useCredentials ? privateKeyData || null : null,
        privateKeyPassphrase: useCredentials ? privateKeyPassphrase || null : null,
        requestId: String(requestId),
      });
      if (requestId !== probeRequestId.current) return;
      setProbe(nextProbe);
      setProbeUrl(nextUrl);
      setProbePhase({ kind: "done" });
      if (!fileName.trim()) {
        setFileName(nextProbe.fileName);
      }
      clearFormError();
    } catch (err) {
      if (requestId !== probeRequestId.current) return;
      log.warn("probe failed", err);
      setFormError(err);
      setProbePhase({ kind: "failed" });
    } finally {
      if (requestId === probeRequestId.current) setProbing(false);
    }
  }

  // UX-6: Subscribe to real probe-phase events from Rust engines.
  // Filters by requestId to discard stale events from previous probes.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onProbePhase((payload) => {
      if (cancelled) return;
      if (payload.requestId !== String(probeRequestId.current)) return;
      setProbePhase({ kind: payload.kind as ProbePhase["kind"] });
    }).then((u) => {
      if (cancelled) {
        u();
        return;
      }
      unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

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
      setProbePhase({ kind: "idle" });
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
    // Batch mode has its own Preview/Create buttons; ignore form submits
    // (e.g. Enter pressed while focused on a non-textarea field).
    if (mode !== "single" || !currentUrl) return;
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
        // F-5: prefer multi-algorithm fields. Legacy `expectedHashSha256` is
        // kept null — backend falls back to it only for older callers. When
        // the chosen algorithm is sha256, also mirror to `expectedHashSha256`
        // so older backend builds (if any) still pick up the digest.
        expectedHash:
          currentIsTorrentProbe || currentIsMetalinkProbe || currentIsHlsProbe || currentIsDashProbe
            ? null
            : expectedHash.trim() || null,
        expectedHashAlgorithm,
        expectedHashSha256:
          currentIsTorrentProbe ||
          currentIsMetalinkProbe ||
          currentIsHlsProbe ||
          currentIsDashProbe ||
          expectedHashAlgorithm !== "sha256"
            ? null
            : expectedHash.trim() || null,
        taskSpeedLimitBps: null,
        priority: null,
        categoryKey: null,
        probeSnapshot: currentProbe,
        selectedFilePaths,
        allowDuplicate,
        username: useCredentials ? username.trim() || null : null,
        password: useCredentials ? password || null : null,
        privateKeyData: useCredentials ? privateKeyData || null : null,
        privateKeyPassphrase: useCredentials ? privateKeyPassphrase || null : null,
        selectedHlsVariantUri: currentIsHlsProbe && selectedHlsVariantUri ? selectedHlsVariantUri : null,
        selectedHlsAudioTrackUris:
          currentIsHlsProbe && selectedHlsAudioTrackUris.length > 0 ? selectedHlsAudioTrackUris : null,
        selectedHlsSubtitleTrackUris:
          currentIsHlsProbe && selectedHlsSubtitleTrackUris.length > 0 ? selectedHlsSubtitleTrackUris : null,
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
    setExpectedHash("");
    setExpectedHashAlgorithm("sha256");
    setProbe(null);
    setProbeUrl("");
    setBatchInput("");
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
    setSelectedHlsAudioTrackUris([]);
    setSelectedHlsSubtitleTrackUris([]);
    setMode("single");
    setUseCredentials(false);
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

  useEffect(() => {
    if (!initialSourceId || appliedInitialSourceId.current === initialSourceId) return;
    appliedInitialSourceId.current = initialSourceId;
    resetForm();

    const nextBatchInput = initialBatchInput?.trim() ? initialBatchInput : "";
    const nextUrl = initialUrl?.trim() ? initialUrl : "";
    if (nextBatchInput) {
      setBatchInput(nextBatchInput);
      setMode("batch");
      void runBatch(false, nextBatchInput);
      return;
    }
    if (nextUrl) {
      setUrl(nextUrl);
      setMode("single");
    }
  }, [initialBatchInput, initialSourceId, initialUrl]);

  useEffect(() => {
    onDraftStateChange?.(
      Boolean(
        url.trim() ||
          saveDir.trim() ||
          fileName.trim() ||
          expectedHash.trim() ||
          batchInput.trim() ||
          selectedLocalFile,
      ),
    );
  }, [batchInput, expectedHash, fileName, onDraftStateChange, saveDir, selectedLocalFile, url]);

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
            {/* Mode segmented control: Single vs Batch. Promotes the batch
                textarea out of the Advanced section so the two task-creation
                paths are equally reachable. */}
            <div
              role="tablist"
              aria-label={t("newDownload.title")}
              className="grid grid-cols-2 gap-1 rounded-md border border-border-subtle bg-surface-raised/40 p-1"
            >
              <button
                type="button"
                role="tab"
                aria-selected={mode === "single"}
                onClick={() => setMode("single")}
                className={`rounded px-3 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary ${
                  mode === "single"
                    ? "bg-surface-base text-text-primary shadow-sm"
                    : "text-text-muted hover:text-text-secondary"
                }`}
              >
                {t("newDownload.modeSingle")}
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === "batch"}
                onClick={() => setMode("batch")}
                className={`rounded px-3 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary ${
                  mode === "batch"
                    ? "bg-surface-base text-text-primary shadow-sm"
                    : "text-text-muted hover:text-text-secondary"
                }`}
              >
                {t("newDownload.modeBatch")}
              </button>
            </div>

            {mode === "single" ? (
              <>
                {/* URL input */}
                <label htmlFor="new-download-url" className="flex flex-col gap-1 text-xs text-text-muted">
                  {t("newDownload.url")}
                  <div className="flex gap-2">
                    <Input
                      id="new-download-url"
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
                <label htmlFor="new-download-save-dir" className="flex flex-col gap-1 text-xs text-text-muted">
                  {t("newDownload.saveDir")}
                  <div className="flex gap-2">
                    <Input
                      id="new-download-save-dir"
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
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="cursor-help rounded bg-surface-raised px-1.5 py-0.5 font-medium text-text-secondary">
                            {protocolBadgeLabel(probe.protocol)}
                          </span>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-60 text-balance">
                          {t(protocolHintKey(probe.protocol))}
                        </TooltipContent>
                      </Tooltip>
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
                          <label className="flex cursor-pointer items-center gap-2 rounded text-xs text-text-secondary transition-colors hover:text-text-primary focus-within:outline-none focus-within:ring-2 focus-within:ring-accent-primary">
                            <Checkbox
                              checked={selectedFiles.size === probe.files.length}
                              indeterminate={selectedFiles.size > 0 && selectedFiles.size < probe.files.length}
                              onChange={toggleAllFiles}
                              aria-label={
                                selectedFiles.size === probe.files.length
                                  ? t("newDownload.deselectAll")
                                  : t("newDownload.selectAll")
                              }
                            />
                            <span>
                              {selectedFiles.size === probe.files.length
                                ? t("newDownload.deselectAll")
                                : t("newDownload.selectAll")}
                            </span>
                          </label>
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
                              className="h-8 text-sm"
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

                {/* Auto-detecting indicator with phase-aware feedback */}
                {!probe && probing ? (
                  <p className="text-xs text-text-muted">
                    {probePhaseMessageKey(probePhase)
                      ? t(probePhaseMessageKey(probePhase) as string)
                      : t("newDownload.autoDetecting")}
                  </p>
                ) : null}

                {/* HLS quality picker */}
                {isHlsProbe && probe && probe.hlsVariants.length > 1 ? (
                  <label htmlFor="new-download-hls-quality" className="flex flex-col gap-1 text-xs text-text-muted">
                    {t("newDownload.hlsQuality")}
                    <Select
                      value={selectedHlsVariantUri ?? ""}
                      onValueChange={(value) => setSelectedHlsVariantUri(value)}
                    >
                      <SelectTrigger id="new-download-hls-quality" className="h-8 w-full text-xs">
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

                {/* F-6: HLS audio track picker */}
                {isHlsProbe && probe && probe.hlsAudioTracks.length > 0 ? (
                  <div className="flex flex-col gap-1 text-xs text-text-muted">
                    <span>{t("newDownload.hlsAudioTracks")}</span>
                    <div className="flex flex-col gap-1">
                      {probe.hlsAudioTracks.map((track) => {
                        const trackUri = track.uri ?? "";
                        const disabled = !track.uri;
                        const checked = selectedHlsAudioTrackUris.includes(trackUri);
                        return (
                          <label
                            key={`${track.groupId}-${track.name}`}
                            className={`flex items-center gap-2 ${disabled ? "opacity-50" : ""}`}
                          >
                            <Checkbox
                              checked={checked}
                              disabled={disabled}
                              onChange={(e) => {
                                if (disabled) return;
                                setSelectedHlsAudioTrackUris((prev) =>
                                  e.target.checked ? [...prev, trackUri] : prev.filter((u) => u !== trackUri),
                                );
                              }}
                              aria-label={track.name}
                            />
                            <span>
                              {track.name}
                              {track.language ? ` (${track.language})` : ""}
                              {track.default ? ` · ${t("newDownload.hlsTrackDefault")}` : ""}
                              {disabled ? ` · ${t("newDownload.hlsTrackEmbedded")}` : ""}
                            </span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                ) : null}

                {/* F-6: HLS subtitle track picker */}
                {isHlsProbe && probe && probe.hlsSubtitleTracks.length > 0 ? (
                  <div className="flex flex-col gap-1 text-xs text-text-muted">
                    <span>{t("newDownload.hlsSubtitleTracks")}</span>
                    <div className="flex flex-col gap-1">
                      {probe.hlsSubtitleTracks.map((track) => {
                        const trackUri = track.uri ?? "";
                        const disabled = !track.uri;
                        const checked = selectedHlsSubtitleTrackUris.includes(trackUri);
                        return (
                          <label
                            key={`${track.groupId}-${track.name}`}
                            className={`flex items-center gap-2 ${disabled ? "opacity-50" : ""}`}
                          >
                            <Checkbox
                              checked={checked}
                              disabled={disabled}
                              onChange={(e) => {
                                if (disabled) return;
                                setSelectedHlsSubtitleTrackUris((prev) =>
                                  e.target.checked ? [...prev, trackUri] : prev.filter((u) => u !== trackUri),
                                );
                              }}
                              aria-label={track.name}
                            />
                            <span>
                              {track.name}
                              {track.language ? ` (${track.language})` : ""}
                              {track.default ? ` · ${t("newDownload.hlsTrackDefault")}` : ""}
                              {disabled ? ` · ${t("newDownload.hlsTrackEmbedded")}` : ""}
                            </span>
                          </label>
                        );
                      })}
                    </div>
                  </div>
                ) : null}

                {/* Credentials disclosure: most URLs are public, so auth is opt-in
                rather than always shown when Advanced is open. The underlying
                state is preserved across toggles; detect/submit pass null when
                this is off. */}
                {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe && !isDashProbe ? (
                  <div className="flex flex-col gap-2 border-t border-border-subtle pt-3">
                    <label className="flex cursor-pointer items-start gap-2">
                      <Checkbox
                        checked={useCredentials}
                        onChange={(event) => setUseCredentials(event.target.checked)}
                        aria-label={t("newDownload.useCredentials")}
                      />
                      <span>
                        <span className="block text-xs font-medium text-text-secondary">
                          {t("newDownload.useCredentials")}
                        </span>
                        <span className="block text-[11px] leading-4 text-text-muted">
                          {t("newDownload.useCredentialsHint")}
                        </span>
                      </span>
                    </label>
                    {useCredentials ? (
                      <div className="flex flex-col gap-2">
                        <div className="grid grid-cols-2 gap-2">
                          <label
                            htmlFor="new-download-username"
                            className="flex flex-col gap-1 text-xs text-text-muted"
                          >
                            {t("newDownload.authUsername")}
                            <Input
                              id="new-download-username"
                              value={username}
                              onChange={(event) => setUsername(event.target.value)}
                              placeholder={t("newDownload.authUsernamePlaceholder")}
                              className="h-8"
                              autoComplete="username"
                            />
                          </label>
                          <label
                            htmlFor="new-download-password"
                            className="flex flex-col gap-1 text-xs text-text-muted"
                          >
                            {t("newDownload.authPassword")}
                            <Input
                              id="new-download-password"
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
                            <span className="text-xs font-medium text-text-secondary">
                              {t("newDownload.sshKeyAuth")}
                            </span>
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
                            <label
                              htmlFor="new-download-ssh-passphrase"
                              className="flex flex-col gap-1 text-xs text-text-muted"
                            >
                              {t("newDownload.sshKeyPassphrase")}
                              <Input
                                id="new-download-ssh-passphrase"
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
                  </div>
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

                {/* Advanced section */}
                {advancedOpen ? (
                  <div className="flex flex-col gap-3 rounded-md border border-border-subtle bg-surface-root/30 p-3">
                    {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe && !isDashProbe ? (
                      <div className="flex flex-col gap-1 text-xs text-text-muted">
                        <label htmlFor="new-download-hash-algorithm">{t("newDownload.hashAlgorithm")}</label>
                        <Select
                          value={expectedHashAlgorithm}
                          onValueChange={(value) => setExpectedHashAlgorithm(value as ChecksumAlgorithm)}
                        >
                          <SelectTrigger id="new-download-hash-algorithm" className="h-8 w-full text-xs">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="sha256">SHA-256</SelectItem>
                            <SelectItem value="sha512">SHA-512</SelectItem>
                            <SelectItem value="sha1">
                              SHA-1{"  "}
                              <span className="text-text-muted">({t("newDownload.weakHash")})</span>
                            </SelectItem>
                            <SelectItem value="md5">
                              MD5{"  "}
                              <span className="text-text-muted">({t("newDownload.weakHash")})</span>
                            </SelectItem>
                          </SelectContent>
                        </Select>
                        <label htmlFor="new-download-hash" className="mt-2">
                          {t("newDownload.expectedHash")}
                        </label>
                        <Input
                          id="new-download-hash"
                          value={expectedHash}
                          onChange={(event) => setExpectedHash(event.target.value)}
                          placeholder={t("newDownload.hashPlaceholder", {
                            algorithm: expectedHashAlgorithm.toUpperCase(),
                          })}
                          className="h-8 font-mono"
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </>
            ) : (
              <>
                {/* Batch import (top-level in batch mode) */}
                <div className="flex flex-col gap-2">
                  <label htmlFor="batch-urls-input" className="text-xs text-text-muted">
                    {t("newDownload.batchUrls")}
                  </label>
                  <textarea
                    id="batch-urls-input"
                    ref={batchInputRef}
                    value={batchInput}
                    onChange={(event) => setBatchInput(event.target.value)}
                    placeholder={t("newDownload.batchUrlsPlaceholder")}
                    className="min-h-32 resize-y rounded-md border border-border-subtle bg-surface-base px-3 py-2 font-mono text-xs text-text-primary outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
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

                {/* Save directory (batch tasks use this via runBatch) */}
                <label htmlFor="new-download-save-dir-batch" className="flex flex-col gap-1 text-xs text-text-muted">
                  {t("newDownload.saveDir")}
                  <div className="flex gap-2">
                    <Input
                      id="new-download-save-dir-batch"
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
              </>
            )}

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
            {mode === "single" ? (
              <Button
                type="submit"
                className="w-full sm:w-auto"
                disabled={submitting || fileSelectionRequired || !url.trim()}
              >
                {submitting ? (
                  <>
                    <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                      <path
                        className="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
                      />
                    </svg>
                    {t("newDownload.starting")}
                  </>
                ) : (
                  t("newDownload.start")
                )}
              </Button>
            ) : null}
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
    <label
      className={`flex w-full cursor-pointer items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-surface-raised/60 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-accent-primary ${
        index > 0 ? "border-t border-border-subtle/50" : ""
      }`}
    >
      <Checkbox checked={checked} onChange={onToggle} aria-label={displayName} />
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
    </label>
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
