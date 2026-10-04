import type { TFunction } from "i18next";
import {
  ChevronDown,
  File,
  FileArchive,
  FileAudio,
  FileImage,
  FileText,
  FileVideo,
  FolderOpen,
  HardDrive,
  Pencil,
  TriangleAlert,
  X,
} from "lucide-react";
import { type FormEvent, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
  HlsMediaTrack,
  ProbedFile,
  ProbeTaskPayload,
  SftpDirectoryProbe,
  TaskPriority,
  TaskProxyMode,
  TaskSource,
  WebDavDirectoryProbe,
} from "@/generated/bindings";
import type { TranslationKey } from "@/i18n";
import {
  applyDraftToCreateTaskInput,
  type CreateDraftShared,
  draftHashFields,
  toDirectoryProbeInput,
  toImportUrlsInput,
  toProbeTaskInput,
} from "@/lib/create-draft";
import { localizedErrorMessage, parseAppError } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { EMPTY_REQUEST_PROFILE, parseRequestProfile } from "@/lib/request-profile";
import { SPEED_LIMIT_UNITS, speedLimitBytesFromInput, speedLimitUnitLabel } from "@/lib/speed-limit";
import {
  createNetworkAuthorization,
  createTask,
  importUrls,
  onProbePhase,
  openDirectoryPicker,
  openFilePicker,
  probeFtpDirectory,
  probeSftpDirectory,
  probeTask,
  probeWebdavDirectory,
  queryDiskSpace,
} from "@/lib/tauri";
import { RequestProfileFields } from "./RequestProfileFields";

const log = createLogger("new-download");

import { getLocalFileKind, pathToFileUrl, readFileAsText } from "@/lib/local-file";
import { cn, formatBytes } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";
import { normalizeTask, parseByteCount } from "@/types/task";
import { BatchImportResults, isFailedBatchItem } from "./BatchImportResults";

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

function probePhaseMessageKey(phase: ProbePhase): TranslationKey | null {
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

function probeErrorHintKey(rawError: unknown, message: string): TranslationKey {
  // Prefer structured error codes from AppErrorPayload when available.
  const appError = parseAppError(rawError);
  if (appError) {
    switch (appError.code) {
      case "dns_failure":
        return "newDownload.probeErrorDns";
      case "http_denied":
      case "ftp_auth_failed":
      case "proxy_auth_failed":
      case "sftp_auth_failed":
      case "sftp_credentials_required":
        return "newDownload.probeErrorDenied";
      case "http_not_found":
      case "sftp_directory_not_file":
        return "newDownload.probeErrorNotFound";
      case "server_rate_limited":
        return "newDownload.probeErrorRateLimited";
      case "timeout":
      case "proxy_timeout":
      case "bt_metadata_timeout":
        return "newDownload.probeErrorTimeout";
      case "connection_refused":
      case "network_unreachable":
      case "proxy_connect_failed":
      case "proxy_connection_failed":
      case "proxy_configuration_invalid":
      case "sftp_connect_failed":
      case "sftp_proxy_connect_failed":
      case "ftp_connect_failed":
      case "bt_source_failed":
      case "bt_metadata_failed":
      case "bt_runtime_stats_failed":
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

/** Badge tooltips. Unknown protocols fall back to the HTTP copy, matching `protocolBadgeLabel`. */
const PROTOCOL_HINT_KEYS: Record<string, TranslationKey | undefined> = {
  bt: "newDownload.protocolHint.bt",
  magnet: "newDownload.protocolHint.magnet",
  metalink: "newDownload.protocolHint.metalink",
  hls: "newDownload.protocolHint.hls",
  dash: "newDownload.protocolHint.dash",
  ftp: "newDownload.protocolHint.ftp",
  ftps: "newDownload.protocolHint.ftps",
  sftp: "newDownload.protocolHint.sftp",
  webdav: "newDownload.protocolHint.webdav",
  webdavs: "newDownload.protocolHint.webdavs",
  http: "newDownload.protocolHint.http",
  https: "newDownload.protocolHint.https",
};

const HTTP_PROTOCOL_HINT_KEY = "newDownload.protocolHint.http" satisfies TranslationKey;

/** i18n key for the plain-language tooltip explaining a protocol badge. */
function protocolHintKey(protocol: string): TranslationKey {
  return PROTOCOL_HINT_KEYS[protocol] ?? HTTP_PROTOCOL_HINT_KEY;
}

function remoteDirectoryEntryKey(entry: RemoteDirectoryEntry): string {
  return "raw" in entry ? `${entry.name}-${entry.raw}` : `${entry.name}-${entry.href}`;
}

function localFileKindLabel(kind: SelectedLocalFile["kind"], t: TFunction): string {
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
  onCloseAutoFocus,
  onCreated,
  initialUrl,
  initialBatchInput,
  initialSourceId,
  onDraftStateChange,
  onCreateStateChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCloseAutoFocus?: (event: Event) => void;
  onCreated: (task: Task) => void;
  initialUrl?: string;
  initialBatchInput?: string;
  initialSourceId?: string;
  onDraftStateChange?: (dirty: boolean) => void;
  onCreateStateChange?: (creating: boolean) => void;
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
  const phaseMessageKey = probePhaseMessageKey(probePhase);
  const [probe, setProbe] = useState<ProbeTaskPayload | null>(null);
  const [probeUrl, setProbeUrl] = useState("");
  const [batchInput, setBatchInput] = useState("");
  const [batchResult, setBatchResult] = useState<BatchImportResult | null>(null);
  const [batchHistory, setBatchHistory] = useState<Array<{ id: string; result: BatchImportResult }>>([]);
  const [batchPreviewing, setBatchPreviewing] = useState(false);
  const [batchCreating, setBatchCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rawError, setRawError] = useState<unknown>(null);
  const [requestProfile, setRequestProfile] = useState(EMPTY_REQUEST_PROFILE);
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
  // FUN-17: shared create-draft fields reachable from both single and batch modes.
  const [priority, setPriority] = useState<TaskPriority>("normal");
  const [startPaused, setStartPaused] = useState(false);
  const [obeySchedule, setObeySchedule] = useState(true);
  const [categoryKey, setCategoryKey] = useState("");
  const [speedAmount, setSpeedAmount] = useState("");
  const [speedUnit, setSpeedUnit] = useState("1048576");
  const [proxyMode, setProxyMode] = useState<TaskProxyMode>("inherit");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyUsername, setProxyUsername] = useState("");
  const [proxyPassword, setProxyPassword] = useState("");
  const [proxyNoProxy, setProxyNoProxy] = useState("");
  const [networkAuthorizationId, setNetworkAuthorizationId] = useState<string | null>(null);
  const [sourceKind, setSourceKind] = useState<TaskSource>("manual");
  const [authorizationRequired, setAuthorizationRequired] = useState(false);

  function buildSharedDraft(options?: {
    allowDuplicate?: boolean;
    skipHash?: boolean;
    networkAuthorizationId?: string | null;
  }): CreateDraftShared {
    const skipHash = options?.skipHash ?? false;
    const hashFields = draftHashFields(expectedHash, expectedHashAlgorithm, skipHash);
    const speedBytes = speedLimitBytesFromInput(speedAmount, speedUnit);
    const taskSpeedLimitBps = speedBytes === undefined || speedBytes == null ? null : String(speedBytes);
    return {
      requestProfile: mode === "single" ? parseRequestProfile(requestProfile) : null,
      networkAuthorizationId: options?.networkAuthorizationId ?? networkAuthorizationId,
      sourceKind: mode === "batch" ? "import" : sourceKind,
      username: useCredentials ? username.trim() || null : null,
      password: useCredentials ? password || null : null,
      privateKeyData: useCredentials ? privateKeyData || null : null,
      privateKeyPassphrase: useCredentials ? privateKeyPassphrase || null : null,
      ...hashFields,
      taskSpeedLimitBps,
      priority,
      categoryKey: categoryKey.trim() || null,
      allowDuplicate: options?.allowDuplicate ?? null,
      proxyMode: proxyMode === "inherit" ? null : proxyMode,
      proxyUrl: proxyMode === "custom" ? proxyUrl.trim() || null : null,
      proxyUsername: proxyMode === "custom" ? proxyUsername.trim() || null : null,
      proxyPassword: proxyMode === "custom" ? proxyPassword || null : null,
      proxyNoProxy: proxyMode === "custom" ? proxyNoProxy.trim() || null : null,
    };
  }

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

  const probeRequestId = useRef("");
  const directoryProbeRequestId = useRef("");
  const latestUrlRef = useRef(url);
  const fileNameEditedRef = useRef(false);
  const batchPreviewOwner = useRef<symbol | null>(null);
  const batchCreateOwner = useRef<symbol | null>(null);
  const batchInputVersion = useRef(0);
  const batchInputRef = useRef<HTMLTextAreaElement | null>(null);
  const appliedInitialSourceId = useRef<string | undefined>(undefined);
  const isTorrentProbe = probe?.protocol === "bt" || probe?.protocol === "magnet";
  const isMetalinkProbe = probe?.protocol === "metalink";
  const isHlsProbe = probe?.protocol === "hls";
  const isDashProbe = probe?.protocol === "dash";
  const isSftpUrl = /^sftp:\/\//i.test(url.trim());
  const hasRequestProfile = Boolean(requestProfile.userAgent || requestProfile.referer || requestProfile.customHeaders);
  const isSelectableMultiFileProbe = isTorrentProbe || isMetalinkProbe;
  const isMultiFile = probe != null && probe.files.length > 1;
  // The folder the file will actually land in: the typed path, else the default.
  const effectiveSaveDir = saveDir.trim() || settings?.defaultSaveDir || "";
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

  latestUrlRef.current = url;

  function clearAutomaticFileName() {
    if (fileNameEditedRef.current) return;
    setFileName("");
    setEditingName(false);
  }

  function setUserFileName(value: string) {
    fileNameEditedRef.current = true;
    setFileName(value);
  }

  function invalidateProbe() {
    probeRequestId.current = "";
    directoryProbeRequestId.current = "";
    setProbe(null);
    setProbeUrl("");
    setProbePhase({ kind: "idle" });
    setRemoteDirectoryProbe(null);
    setProbing(false);
    setRemoteDirectoryLoading(false);
    setAuthorizationRequired(false);
  }

  function changeUrl(nextUrl: string) {
    if (nextUrl === url) {
      setUrl(nextUrl);
      return;
    }
    // Invalidate synchronously from the input handler. Waiting for the URL
    // effect would leave a small window where an old response could win.
    latestUrlRef.current = nextUrl;
    invalidateProbe();
    clearAutomaticFileName();
    setSubmitStatus(null);
    setDuplicateOverrideAvailable(false);
    setNetworkAuthorizationId(null);
    setUrl(nextUrl);
  }

  async function authorizeCurrentTarget() {
    const target = url.trim();
    if (!target) return;
    setSubmitting(true);
    clearFormError();
    try {
      const draft = await createNetworkAuthorization(target, sourceKind);
      setNetworkAuthorizationId(draft.id);
      setAuthorizationRequired(false);
      await detect(target, false);
    } catch (err) {
      setFormError(err);
    } finally {
      setSubmitting(false);
    }
  }

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
    // URL changes already advance the session before the debounce fires. A
    // manual re-probe gets a fresh id so two same-URL requests cannot collide.
    const requestId = crypto.randomUUID();
    probeRequestId.current = requestId;
    setProbing(true);
    setProbePhase(inferProbePhaseFromUrl(nextUrl));
    setDuplicateOverrideAvailable(false);
    if (!automatic) clearFormError();
    setProbe(null);
    setProbeUrl("");
    try {
      const draft = buildSharedDraft({ skipHash: true });
      const nextProbe = await probeTask(toProbeTaskInput(nextUrl, draft, String(requestId)));
      if (requestId !== probeRequestId.current || latestUrlRef.current.trim() !== nextUrl.trim()) return;
      setProbe(nextProbe);
      setProbeUrl(nextUrl);
      setProbePhase({ kind: "done" });
      if (!fileNameEditedRef.current) {
        setFileName(nextProbe.fileName);
      }
      clearFormError();
    } catch (err) {
      if (requestId !== probeRequestId.current || latestUrlRef.current.trim() !== nextUrl.trim()) return;
      log.warn("probe failed", err);
      setFormError(err);
      setAuthorizationRequired(parseAppError(err)?.code === "intranet_target_blocked");
      setProbePhase({ kind: "failed" });
    } finally {
      if (requestId === probeRequestId.current) {
        setProbing(false);
        probeRequestId.current = "";
      }
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
    const requestId = crypto.randomUUID();
    directoryProbeRequestId.current = requestId;
    setRemoteDirectoryLoading(true);
    setError(null);
    try {
      const draft = buildSharedDraft({ skipHash: true });
      const input = toDirectoryProbeInput(nextUrl, draft);
      const result = /^webdavs?:\/\//i.test(nextUrl)
        ? await probeWebdavDirectory(input)
        : /^sftp:\/\//i.test(nextUrl)
          ? await probeSftpDirectory(input)
          : await probeFtpDirectory(input);
      if (requestId !== directoryProbeRequestId.current || latestUrlRef.current.trim() !== nextUrl.trim()) return;
      setRemoteDirectoryProbe(result);
    } catch (err) {
      if (requestId === directoryProbeRequestId.current && latestUrlRef.current.trim() === nextUrl.trim()) {
        setError(localizedErrorMessage(err, t));
        setAuthorizationRequired(parseAppError(err)?.code === "intranet_target_blocked");
      }
    } finally {
      if (requestId === directoryProbeRequestId.current) setRemoteDirectoryLoading(false);
    }
  }

  const probeContext = JSON.stringify([
    requestProfile,
    open,
    mode,
    url,
    useCredentials,
    username,
    password,
    privateKeyData,
    privateKeyPassphrase,
    proxyMode,
    proxyUrl,
    proxyUsername,
    proxyPassword,
    proxyNoProxy,
  ]);
  const batchContext = JSON.stringify([
    saveDir,
    expectedHash,
    expectedHashAlgorithm,
    priority,
    categoryKey,
    speedAmount,
    speedUnit,
    useCredentials,
    username,
    password,
    privateKeyData,
    privateKeyPassphrase,
    proxyMode,
    proxyUrl,
    proxyUsername,
    proxyPassword,
    proxyNoProxy,
    obeySchedule,
  ]);
  const latestDetect = useRef(detect);
  latestDetect.current = detect;

  // Invalidate at commit, including programmatic edits and close/reopen. UUIDs
  // prevent events from a prior mount matching this dialog's active request.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the serialized context is the complete resource identity.
  useLayoutEffect(() => {
    invalidateProbe();
    clearAutomaticFileName();
    clearFormError();
    return () => {
      probeRequestId.current = "";
      directoryProbeRequestId.current = "";
    };
  }, [probeContext]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: restart for each context change and read the latest draft at dispatch.
  useEffect(() => {
    if (!open || mode !== "single" || !url.trim() || canProbeRemoteDirectory) return;
    const timeoutId = window.setTimeout(() => void latestDetect.current(url.trim(), true), 650);
    return () => window.clearTimeout(timeoutId);
  }, [probeContext]);

  function invalidateBatchPreview() {
    batchInputVersion.current += 1;
    batchPreviewOwner.current = null;
    setBatchPreviewing(false);
    setBatchResult(null);
  }

  function changeBatchInput(input: string) {
    invalidateBatchPreview();
    setBatchInput(input);
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: all shared creation fields participate in the batch version.
  useLayoutEffect(() => {
    invalidateBatchPreview();
  }, [batchContext, open]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await submitCurrent(false, false);
  }

  async function submitDuplicateOverride() {
    await submitCurrent(true, startPaused);
  }

  async function submitCurrent(allowDuplicate: boolean, nextStartPaused: boolean) {
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
    setStartPaused(nextStartPaused);
    setError(null);
    setDuplicateOverrideAvailable(false);
    setSubmitStatus(currentProbe ? t("newDownload.usingProbe") : t("newDownload.revalidating"));
    try {
      const draft = buildSharedDraft({
        allowDuplicate,
        skipHash: currentIsTorrentProbe || currentIsMetalinkProbe || currentIsHlsProbe || currentIsDashProbe,
      });
      const task = await createTask(
        applyDraftToCreateTaskInput(
          {
            url: currentUrl,
            saveDir: saveDir.trim() || null,
            fileName: fileName.trim() || null,
            startPaused: nextStartPaused,
            obeySchedule,
            probeSnapshot: currentProbe,
            selectedFilePaths,
            selectedHlsVariantUri: currentIsHlsProbe && selectedHlsVariantUri ? selectedHlsVariantUri : null,
            selectedHlsAudioTrackUris:
              currentIsHlsProbe && selectedHlsAudioTrackUris.length > 0 ? selectedHlsAudioTrackUris : null,
            selectedHlsSubtitleTrackUris:
              currentIsHlsProbe && selectedHlsSubtitleTrackUris.length > 0 ? selectedHlsSubtitleTrackUris : null,
          },
          draft,
        ),
      );
      onCreated(task);
      resetForm();
      onOpenChange(false);
    } catch (err) {
      log.error("create task failed", err);
      const appError = parseAppError(err);
      setDuplicateOverrideAvailable(appError?.code === "duplicate_task");
      setAuthorizationRequired(appError?.code === "intranet_target_blocked");
      setError(localizedErrorMessage(err, t));
    } finally {
      setSubmitting(false);
      setSubmitStatus(null);
    }
  }

  function resetForm() {
    probeRequestId.current = "";
    invalidateBatchPreview();
    batchInputVersion.current += 1;
    directoryProbeRequestId.current = "";
    fileNameEditedRef.current = false;
    latestUrlRef.current = "";
    setUrl("");
    setNetworkAuthorizationId(null);
    setSourceKind("manual");
    setSaveDir("");
    setFileName("");
    setExpectedHash("");
    setExpectedHashAlgorithm("sha256");
    setProbe(null);
    setProbeUrl("");
    setProbing(false);
    setBatchInput("");
    setBatchResult(null);

    setRemoteDirectoryProbe(null);
    setDuplicateOverrideAvailable(false);
    setSubmitStatus(null);
    setAdvancedOpen(false);
    setSelectedLocalFile(null);
    setSelectedFiles(new Set());
    setEditingName(false);
    setRequestProfile(EMPTY_REQUEST_PROFILE);
    setUsername("");
    setPassword("");
    setPrivateKeyData("");
    setPrivateKeyPassphrase("");
    setSelectedHlsVariantUri(null);
    setSelectedHlsAudioTrackUris([]);
    setSelectedHlsSubtitleTrackUris([]);
    setMode("single");
    setUseCredentials(false);
    setPriority("normal");
    setStartPaused(false);
    setObeySchedule(true);
    setCategoryKey("");
    setSpeedAmount("");
    setSpeedUnit("1048576");
    setProxyMode("inherit");
    setProxyUrl("");
    setProxyUsername("");
    setProxyPassword("");
    setProxyNoProxy("");
  }

  async function chooseDirectory() {
    // UX-25: a rejected picker must not become a silent unhandled rejection;
    // surface it in the dialog's error region like the sibling pickers.
    try {
      const selected = await openDirectoryPicker();
      if (selected) setSaveDir(selected);
    } catch (err) {
      log.error("directory picker failed", err);
      setError(localizedErrorMessage(err, t));
    }
  }

  async function chooseLocalFile() {
    setFileLoading(true);
    try {
      const picked = await openFilePicker([
        { name: t("newDownload.manifestTextFilter"), extensions: ["torrent", "meta4", "metalink", "mpd", "txt"] },
      ]);
      if (!picked) return;

      const kind = getLocalFileKind(picked.name);
      const file: SelectedLocalFile = { path: picked.path, name: picked.name, kind };
      setSelectedLocalFile(file);

      if (kind === "torrent" || kind === "metalink" || kind === "dash") {
        const fileUrl = pathToFileUrl(picked.path);
        changeUrl(fileUrl);
      } else {
        try {
          const text = await readFileAsText(picked.path, "batch_text");
          // UX-04: enter batch mode and preview immediately, matching handoff.
          changeBatchInput(text);
          setMode("batch");
          void runBatch(false, text);
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
        {
          name: t("newDownload.sshPrivateKeyFilter"),
          extensions: ["pem", "key", "id_rsa", "id_ed25519", "id_ecdsa", ""],
        },
      ]);
      if (!picked) return;
      const content = await readFileAsText(picked.path, "ssh_key");
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
      changeUrl("");
    }
  }

  async function runBatch(
    create: boolean,
    inputOverride?: string,
    action?: { batchId: string; indices: number[]; allowDuplicate?: boolean },
  ) {
    const input = inputOverride ?? batchInput;
    const currentBatchInput = batchInput;
    // Refs exclude same-tick clicks and alternate entry points before React
    // renders the disabled controls. An edit can supersede a read-only preview.
    if (!input.trim() || batchCreateOwner.current || batchPreviewOwner.current) return;
    const owner = Symbol();
    const version = batchInputVersion.current;
    if (create) {
      batchCreateOwner.current = owner;
      setBatchCreating(true);
    } else {
      batchPreviewOwner.current = owner;
      setBatchPreviewing(true);
    }
    setError(null);
    try {
      const draft = buildSharedDraft({ allowDuplicate: action?.allowDuplicate ?? false });
      const result = await importUrls(toImportUrlsInput(input, saveDir.trim() || null, create, draft, obeySchedule));
      if (create) {
        // Creation has durable side effects even if the draft changes during
        // IPC. Always retain its outcome and notify the task store.
        setBatchHistory((history) =>
          action
            ? history.map((batch) =>
                batch.id !== action.batchId
                  ? batch
                  : {
                      ...batch,
                      result: mergeBatchRetry(batch.result, action.indices, result),
                    },
              )
            : [...history, { id: crypto.randomUUID(), result }],
        );
        for (const item of result.items) if (item.task) onCreated(normalizeTask(item.task));
        if (!action && version === batchInputVersion.current) {
          changeBatchInput(removeCreatedBatchLines(input, result));
        } else if (action?.allowDuplicate && version === batchInputVersion.current) {
          changeBatchInput(
            removeBatchUrls(
              currentBatchInput,
              result.items.filter((item) => item.task).map((item) => item.inputUrl),
            ),
          );
        }
      } else if (owner === batchPreviewOwner.current && version === batchInputVersion.current) {
        setBatchResult(result);
      }
    } catch (err) {
      if (create || (owner === batchPreviewOwner.current && version === batchInputVersion.current)) {
        setError(localizedErrorMessage(err, t));
      }
    } finally {
      if (create && batchCreateOwner.current === owner) {
        batchCreateOwner.current = null;
        setBatchCreating(false);
      }
      if (!create && batchPreviewOwner.current === owner) {
        batchPreviewOwner.current = null;
        setBatchPreviewing(false);
      }
    }
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: initialSourceId is the single-use handoff identity; callbacks must not replay it.
  useEffect(() => {
    if (!initialSourceId || appliedInitialSourceId.current === initialSourceId) return;
    appliedInitialSourceId.current = initialSourceId;
    resetForm();

    const nextBatchInput = initialBatchInput?.trim() ? initialBatchInput : "";
    const nextUrl = initialUrl?.trim() ? initialUrl : "";
    if (nextBatchInput) {
      setSourceKind("import");
      changeBatchInput(nextBatchInput);
      setMode("batch");
      void runBatch(false, nextBatchInput);
      return;
    }
    if (nextUrl) {
      setSourceKind(
        initialSourceId === "clipboard" || initialSourceId.startsWith("clipboard-")
          ? "clipboard"
          : initialSourceId.startsWith("browser")
            ? "browser"
            : "manual",
      );
      changeUrl(nextUrl);
      setMode("single");
    }
  }, [initialBatchInput, initialSourceId, initialUrl]);

  // UX-30: this guard also protects the kept draft from clipboard and file-drop
  // handoff in AppShell. Every non-default creation override must participate,
  // because an incoming source resets the form and would otherwise erase it.
  const draftDirty = Boolean(
    url.trim() ||
      saveDir.trim() ||
      fileName.trim() ||
      expectedHash.trim() ||
      expectedHashAlgorithm !== "sha256" ||
      batchInput.trim() ||
      selectedLocalFile ||
      requestProfile.userAgent ||
      requestProfile.referer ||
      requestProfile.customHeaders ||
      useCredentials ||
      username.trim() ||
      password ||
      privateKeyData ||
      privateKeyPassphrase ||
      priority !== "normal" ||
      categoryKey.trim() ||
      speedAmount.trim() ||
      proxyMode !== "inherit" ||
      proxyUrl.trim() ||
      proxyUsername.trim() ||
      proxyPassword ||
      proxyNoProxy.trim() ||
      !obeySchedule ||
      selectedHlsVariantUri ||
      selectedHlsAudioTrackUris.length > 0 ||
      selectedHlsSubtitleTrackUris.length > 0,
  );

  useEffect(() => {
    onDraftStateChange?.(draftDirty);
  }, [draftDirty, onDraftStateChange]);

  // Closing while a create is in flight hides the dialog but must not suggest
  // the request was canceled — the IPC keeps running and the task appears in
  // the list when done. Closing a dirty draft keeps it in session memory
  // (the shell keeps this component mounted) and says so explicitly.
  const createInFlight = submitting || batchCreating;
  const batchCreateDisabled = batchCreating || batchPreviewing || !batchInput.trim();

  useEffect(() => {
    onCreateStateChange?.(createInFlight);
  }, [createInFlight, onCreateStateChange]);

  function handleOpenChange(nextOpen: boolean) {
    if (nextOpen) {
      onOpenChange(true);
      return;
    }
    if (createInFlight) {
      onOpenChange(false);
      useToastStore.getState().addToast({
        tone: "info",
        title: t("newDownload.closeSubmittingTitle"),
        description: t("newDownload.closeSubmittingDescription"),
        key: "new-download-busy-close",
      });
      return;
    }
    if (draftDirty) {
      onOpenChange(false);
      useToastStore.getState().addToast({
        tone: "info",
        title: t("newDownload.draftKeptTitle"),
        description: t("newDownload.draftKeptDescription"),
        key: "new-download-draft-kept",
      });
      return;
    }
    onOpenChange(false);
  }

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
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent onCloseAutoFocus={onCloseAutoFocus}>
        <DialogHeader>
          <DialogTitle>{t("newDownload.title")}</DialogTitle>
          <DialogDescription className="sr-only">{t("newDownload.description")}</DialogDescription>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col overflow-hidden" onSubmit={submit}>
          <DialogBody className="flex flex-col gap-3 py-4">
            {/* Mode segmented control: Single vs Batch. Promotes the batch
                textarea out of the Advanced section so the two task-creation
                paths are equally reachable. */}
            <fieldset className="m-0 grid min-w-0 grid-cols-2 gap-1 rounded-md border border-border-subtle bg-surface-raised/40 p-1">
              <legend className="sr-only">{t("newDownload.title")}</legend>
              <button
                type="button"
                aria-pressed={mode === "single"}
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
                aria-pressed={mode === "batch"}
                onClick={() => setMode("batch")}
                className={`rounded px-3 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary ${
                  mode === "batch"
                    ? "bg-surface-base text-text-primary shadow-sm"
                    : "text-text-muted hover:text-text-secondary"
                }`}
              >
                {t("newDownload.modeBatch")}
              </button>
            </fieldset>

            {mode === "single" ? (
              <>
                {/* URL input */}
                <div className="flex flex-col gap-1 text-xs text-text-muted">
                  <label htmlFor="new-download-url">{t("newDownload.url")}</label>
                  <div className="flex gap-2">
                    <Input
                      id="new-download-url"
                      value={url}
                      aria-invalid={!!error}
                      aria-describedby={error ? "new-download-error" : undefined}
                      onChange={(event) => {
                        changeUrl(event.target.value);
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
                </div>

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

                {authorizationRequired ? (
                  <div className="rounded-md border border-border-warning bg-status-warning/10 px-3 py-2 text-xs text-status-warning">
                    <p className="font-medium">{t("newDownload.intranetAuthorizationRequired")}</p>
                    <p className="mt-1 text-[11px]">{t("newDownload.intranetAuthorizationHint")}</p>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="mt-2 h-8"
                      disabled={submitting || probing}
                      onClick={() => void authorizeCurrentTarget()}
                    >
                      {t("newDownload.authorizeTarget")}
                    </Button>
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
                              changeUrl(entry.probableFileUrl);
                              setRemoteDirectoryProbe(null);
                            }
                          }}
                        >
                          <span className="truncate text-text-secondary" title={entry.name}>
                            {entry.name}
                          </span>
                          <span className="shrink-0 text-text-muted">
                            {entry.probableFileUrl ? t("newDownload.useFileUrl") : t("newDownload.directoryEntry")}
                          </span>
                        </button>
                      ))}
                    </div>
                    {remoteDirectoryProbe.diagnostics.length > 0 ? (
                      <p className="mt-2 truncate text-text-muted" title={remoteDirectoryProbe.diagnostics[0]}>
                        {remoteDirectoryProbe.diagnostics[0]}
                      </p>
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
                      <p className="truncate text-sm text-text-primary" title={selectedLocalFile.name}>
                        {selectedLocalFile.name}
                      </p>
                      <p className="text-xs text-text-muted">{localFileKindLabel(selectedLocalFile.kind, t)}</p>
                    </div>
                    <button
                      type="button"
                      onClick={clearSelectedLocalFile}
                      className="shrink-0 rounded p-1.5 min-h-8 min-w-8 text-text-muted transition-colors hover:bg-surface-raised hover:text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                      aria-label={t("newDownload.removeFile")}
                      title={t("newDownload.removeFile")}
                    >
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                ) : null}

                {/* Save directory */}
                <div className="flex flex-col gap-1 text-xs text-text-muted">
                  <label htmlFor="new-download-save-dir">{t("newDownload.saveDir")}</label>
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
                      aria-label={t("newDownload.chooseDirectory")}
                      title={t("newDownload.chooseDirectory")}
                    >
                      <FolderOpen className="h-4 w-4" />
                    </Button>
                  </div>
                </div>

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
                          <label
                            htmlFor="new-download-select-all-files"
                            className="flex cursor-pointer items-center gap-2 rounded text-xs text-text-secondary transition-colors hover:text-text-primary focus-within:outline-none focus-within:ring-2 focus-within:ring-accent-primary"
                          >
                            <Checkbox
                              id="new-download-select-all-files"
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
                        <div className="flex items-center justify-between gap-3 border-t border-border-subtle px-3 py-2 text-xs text-text-muted">
                          <span>{t("newDownload.totalSize")}</span>
                          <span className="flex min-w-0 items-center gap-3">
                            <FreeSpaceNote dir={effectiveSaveDir} neededBytes={selectedTotal} />
                            <span className="font-mono text-text-primary">{formatBytes(selectedTotal)}</span>
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
                              onChange={(e) => setUserFileName(e.target.value)}
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
                              <span className="truncate text-sm text-text-primary" title={fileName || probe.fileName}>
                                {fileName || probe.fileName}
                              </span>
                              {/* Always visible: a hover-only pencil hid the one way to rename before starting. */}
                              <Pencil className="h-3 w-3 shrink-0 text-text-muted transition-colors group-hover:text-text-primary" />
                            </button>
                          )}
                          {/* Resume support is already stated in the header strip; this
                              line answers the next question for a large file: will it fit? */}
                          <div className="mt-0.5 flex min-w-0 items-center gap-3 text-xs text-text-muted">
                            <span className="shrink-0 font-mono tabular-nums">
                              {formatBytes(parseByteCount(probe.totalSize))}
                            </span>
                            <FreeSpaceNote dir={effectiveSaveDir} neededBytes={parseByteCount(probe.totalSize)} />
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                ) : null}

                {/* Auto-detecting indicator with phase-aware feedback */}
                {!probe && probing ? (
                  <p className="text-xs text-text-muted">
                    {phaseMessageKey ? t(phaseMessageKey) : t("newDownload.autoDetecting")}
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

                {/* F-6: HLS audio track picker (capped, see HlsTrackPicker) */}
                {isHlsProbe && probe && probe.hlsAudioTracks.length > 0 ? (
                  <HlsTrackPicker
                    heading={t("newDownload.hlsAudioTracks")}
                    tracks={probe.hlsAudioTracks}
                    selectedUris={selectedHlsAudioTrackUris}
                    onToggle={setSelectedHlsAudioTrackUris}
                    idPrefix="new-download-hls-audio"
                  />
                ) : null}

                {/* F-6: HLS subtitle track picker (capped, see HlsTrackPicker) */}
                {isHlsProbe && probe && probe.hlsSubtitleTracks.length > 0 ? (
                  <HlsTrackPicker
                    heading={t("newDownload.hlsSubtitleTracks")}
                    tracks={probe.hlsSubtitleTracks}
                    selectedUris={selectedHlsSubtitleTrackUris}
                    onToggle={setSelectedHlsSubtitleTrackUris}
                    idPrefix="new-download-hls-subtitle"
                  />
                ) : null}

                {/* Advanced options toggle */}
                <button
                  type="button"
                  onClick={() => setAdvancedOpen((v) => !v)}
                  aria-expanded={advancedOpen}
                  aria-controls="new-download-advanced-options"
                  className="flex items-center gap-1.5 self-start text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                >
                  <ChevronDown
                    className={`h-3.5 w-3.5 transition-transform duration-200 ${advancedOpen ? "" : "-rotate-90"}`}
                  />
                  {t("newDownload.advancedOptions")}
                </button>

                {/* Advanced section */}
                {advancedOpen ? (
                  <div
                    id="new-download-advanced-options"
                    className="flex flex-col gap-3 rounded-md border border-border-subtle bg-surface-root/30 p-3"
                  >
                    {hasRequestProfile ||
                    (/^https?:\/\//i.test(url.trim()) &&
                      !isTorrentProbe &&
                      !/\.torrent(?:[?#].*)?$/i.test(url.trim())) ? (
                      <RequestProfileFields
                        idPrefix="new-download-request"
                        value={requestProfile}
                        disabled={submitting}
                        onChange={(next) => {
                          invalidateProbe();
                          setProbeUrl("");
                          setRequestProfile(next);
                          setDuplicateOverrideAvailable(false);
                          clearFormError();
                        }}
                      />
                    ) : null}
                    {/* Credentials sit behind the Advanced gate like every other
                    override: most URLs are public, and probe failures that need
                    auth surface the denied hint above. The underlying state is
                    preserved across toggles; detect/submit pass null when off. */}
                    {!isTorrentProbe && !isMetalinkProbe && !isHlsProbe && !isDashProbe ? (
                      <div className="flex flex-col gap-2">
                        <label htmlFor="new-download-use-credentials" className="flex cursor-pointer items-start gap-2">
                          <Checkbox
                            id="new-download-use-credentials"
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
                                      aria-label={t("newDownload.sshKeyClear")}
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

                    <SharedCreateDraftFields
                      priority={priority}
                      setPriority={setPriority}
                      categoryKey={categoryKey}
                      setCategoryKey={setCategoryKey}
                      speedAmount={speedAmount}
                      setSpeedAmount={setSpeedAmount}
                      speedUnit={speedUnit}
                      setSpeedUnit={setSpeedUnit}
                      proxyMode={proxyMode}
                      setProxyMode={setProxyMode}
                      proxyUrl={proxyUrl}
                      setProxyUrl={setProxyUrl}
                      proxyUsername={proxyUsername}
                      setProxyUsername={setProxyUsername}
                      proxyPassword={proxyPassword}
                      setProxyPassword={setProxyPassword}
                      proxyNoProxy={proxyNoProxy}
                      setProxyNoProxy={setProxyNoProxy}
                    />
                    <label htmlFor="new-download-obey-schedule" className="flex cursor-pointer items-start gap-2">
                      <Checkbox
                        id="new-download-obey-schedule"
                        checked={obeySchedule}
                        onChange={(event) => setObeySchedule(event.target.checked)}
                        aria-label={t("newDownload.obeySchedule")}
                      />
                      <span>
                        <span className="block text-xs font-medium text-text-secondary">
                          {t("newDownload.obeySchedule")}
                        </span>
                        <span className="block text-[11px] leading-4 text-text-muted">
                          {t("newDownload.obeyScheduleHint")}
                        </span>
                      </span>
                    </label>
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
                    onChange={(event) => {
                      changeBatchInput(event.target.value);
                    }}
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
                      disabled={batchPreviewing || batchCreating || !batchInput.trim()}
                    >
                      {t("newDownload.previewBatch")}
                    </Button>
                  </div>
                  {(batchPreviewing || batchCreating) && (
                    <p role="status" className="text-xs text-text-muted">
                      {t(batchCreating ? "newDownload.batchCreating" : "newDownload.batchPreviewing")}
                    </p>
                  )}
                  {batchHistory.map((batch) => (
                    <BatchImportResults
                      key={batch.id}
                      result={batch.result}
                      busy={batchPreviewing || batchCreating}
                      onRetry={(indices, urls) => void runBatch(true, urls.join("\n"), { batchId: batch.id, indices })}
                      onCreateDuplicates={(indices, urls) =>
                        void runBatch(true, urls.join("\n"), {
                          batchId: batch.id,
                          indices,
                          allowDuplicate: true,
                        })
                      }
                    />
                  ))}
                  {batchResult && <BatchImportResults result={batchResult} busy={batchPreviewing || batchCreating} />}
                </div>

                {/* Save directory (batch tasks use this via runBatch) */}
                <div className="flex flex-col gap-1 text-xs text-text-muted">
                  <label htmlFor="new-download-save-dir-batch">{t("newDownload.saveDir")}</label>
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
                      aria-label={t("newDownload.chooseDirectory")}
                      title={t("newDownload.chooseDirectory")}
                    >
                      <FolderOpen className="h-4 w-4" />
                    </Button>
                  </div>
                </div>

                <button
                  type="button"
                  onClick={() => setAdvancedOpen((v) => !v)}
                  aria-expanded={advancedOpen}
                  aria-controls="new-download-batch-options"
                  className="flex items-center gap-1.5 self-start text-xs text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
                >
                  <ChevronDown
                    className={`h-3.5 w-3.5 transition-transform duration-200 ${advancedOpen ? "" : "-rotate-90"}`}
                  />
                  {t("newDownload.advancedOptions")}
                </button>

                {/* FUN-17: batch shares the same create-draft overrides as single create. */}
                {advancedOpen ? (
                  <div
                    id="new-download-batch-options"
                    className="flex flex-col gap-3 rounded-md border border-border-subtle bg-surface-root/30 p-3"
                  >
                    <label
                      htmlFor="new-download-use-credentials-batch"
                      className="flex cursor-pointer items-start gap-2"
                    >
                      <Checkbox
                        id="new-download-use-credentials-batch"
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
                      <div className="grid grid-cols-2 gap-2">
                        <label
                          htmlFor="new-download-username-batch"
                          className="flex flex-col gap-1 text-xs text-text-muted"
                        >
                          {t("newDownload.authUsername")}
                          <Input
                            id="new-download-username-batch"
                            value={username}
                            onChange={(event) => setUsername(event.target.value)}
                            className="h-8"
                            autoComplete="username"
                          />
                        </label>
                        <label
                          htmlFor="new-download-password-batch"
                          className="flex flex-col gap-1 text-xs text-text-muted"
                        >
                          {t("newDownload.authPassword")}
                          <Input
                            id="new-download-password-batch"
                            type="password"
                            value={password}
                            onChange={(event) => setPassword(event.target.value)}
                            className="h-8"
                            autoComplete="current-password"
                          />
                        </label>
                      </div>
                    ) : null}
                    <SharedCreateDraftFields
                      priority={priority}
                      setPriority={setPriority}
                      categoryKey={categoryKey}
                      setCategoryKey={setCategoryKey}
                      speedAmount={speedAmount}
                      setSpeedAmount={setSpeedAmount}
                      speedUnit={speedUnit}
                      setSpeedUnit={setSpeedUnit}
                      proxyMode={proxyMode}
                      setProxyMode={setProxyMode}
                      proxyUrl={proxyUrl}
                      setProxyUrl={setProxyUrl}
                      proxyUsername={proxyUsername}
                      setProxyUsername={setProxyUsername}
                      proxyPassword={proxyPassword}
                      setProxyPassword={setProxyPassword}
                      proxyNoProxy={proxyNoProxy}
                      setProxyNoProxy={setProxyNoProxy}
                    />
                    <label htmlFor="new-download-obey-schedule-batch" className="flex cursor-pointer items-start gap-2">
                      <Checkbox
                        id="new-download-obey-schedule-batch"
                        checked={obeySchedule}
                        onChange={(event) => setObeySchedule(event.target.checked)}
                        aria-label={t("newDownload.obeySchedule")}
                      />
                      <span>
                        <span className="block text-xs font-medium text-text-secondary">
                          {t("newDownload.obeySchedule")}
                        </span>
                        <span className="block text-[11px] leading-4 text-text-muted">
                          {t("newDownload.obeyScheduleHint")}
                        </span>
                      </span>
                    </label>
                  </div>
                ) : null}
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
                id="new-download-error"
                role="alert"
                aria-live="polite"
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
              onClick={() => handleOpenChange(false)}
              disabled={createInFlight}
            >
              {t("newDownload.cancel")}
            </Button>
            {mode === "single" ? (
              <>
                <Button
                  type="button"
                  variant="outline"
                  className="w-full sm:w-auto"
                  disabled={submitting || fileSelectionRequired || !url.trim()}
                  onClick={() => void submitCurrent(false, true)}
                >
                  {submitting && startPaused ? (
                    <>
                      <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                        <path
                          className="opacity-75"
                          fill="currentColor"
                          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
                        />
                      </svg>
                      {t("newDownload.adding")}
                    </>
                  ) : (
                    t("newDownload.addPaused")
                  )}
                </Button>
                <Button
                  type="submit"
                  className="w-full sm:w-auto"
                  disabled={submitting || fileSelectionRequired || !url.trim()}
                >
                  {submitting && !startPaused ? (
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
              </>
            ) : (
              <Button
                type="button"
                className="w-full sm:w-auto"
                disabled={batchCreateDisabled}
                onClick={() => void runBatch(true)}
              >
                {t("newDownload.createBatch")}
              </Button>
            )}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/* ------------------------------------------------------------------ */
/*  Sub-components                                                      */
/* ------------------------------------------------------------------ */

/**
 * Free space in the target folder, checked before Start instead of after a
 * multi-gigabyte download fails at 90%. Unknown sizes and failed queries show
 * nothing rather than a guess.
 */
function FreeSpaceNote({ dir, neededBytes }: { dir: string; neededBytes: number }) {
  const { t } = useTranslation();
  const [available, setAvailable] = useState<number | null>(null);

  useEffect(() => {
    setAvailable(null);
    if (!dir) return;
    let cancelled = false;
    // Typing a path fires this per keystroke; wait for the input to settle.
    const timer = window.setTimeout(() => {
      queryDiskSpace(dir)
        .then((info) => {
          const bytes = Number(info.available_bytes);
          if (!cancelled && Number.isFinite(bytes)) setAvailable(bytes);
        })
        .catch((error) => log.debug("free space query failed", error));
    }, 300);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [dir]);

  if (available == null) return null;
  const short = neededBytes > 0 && neededBytes > available;
  return (
    <span
      className={cn("flex min-w-0 items-center gap-1", short ? "font-medium text-status-warning" : "text-text-muted")}
    >
      {short ? (
        <TriangleAlert className="h-3 w-3 shrink-0" aria-hidden />
      ) : (
        <HardDrive className="h-3 w-3 shrink-0" aria-hidden />
      )}
      <span className="truncate">
        {short
          ? t("newDownload.freeSpaceShort", { needed: formatBytes(neededBytes), available: formatBytes(available) })
          : t("newDownload.freeSpace", { available: formatBytes(available) })}
      </span>
    </span>
  );
}

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
      htmlFor={`new-download-file-${index}`}
      className={`flex w-full cursor-pointer items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-surface-raised/60 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-accent-primary ${
        index > 0 ? "border-t border-border-subtle/50" : ""
      }`}
    >
      <Checkbox id={`new-download-file-${index}`} checked={checked} onChange={onToggle} aria-label={displayName} />
      <span className="shrink-0 text-text-muted">{fileIcon(displayName)}</span>
      <div className="min-w-0 flex-1">
        <p className={`truncate text-sm ${checked ? "text-text-primary" : "text-text-muted"}`} title={displayName}>
          {displayName}
        </p>
        {hasPath ? (
          <p className="truncate text-[10px] text-text-muted" title={file.relativePath}>
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

/* ------------------------------------------------------------------ */
/*  HLS track picker                                                    */
/* ------------------------------------------------------------------ */

// Cap the always-rendered track rows. Long-tail manifests (sports and event
// streams) carry hundreds of audio/subtitle renditions; an unbounded checkbox
// wall made the dialog unusable long before the submit button. 6 keeps every
// realistic single-language media fully visible without a toggle.
const HLS_TRACK_VISIBLE_LIMIT = 6;

function HlsTrackPicker({
  heading,
  tracks,
  selectedUris,
  onToggle,
  idPrefix,
}: {
  heading: string;
  tracks: HlsMediaTrack[];
  selectedUris: string[];
  onToggle: (update: (prev: string[]) => string[]) => void;
  idPrefix: string;
}) {
  const { t } = useTranslation();
  const [showAll, setShowAll] = useState(false);
  // Collapsed keeps selected tracks visible beyond the cap, so an auto-selected
  // default never hides its own checked state.
  const visibleTracks = tracks
    .map((track, index) => ({ track, index }))
    .filter(
      ({ track, index }) =>
        showAll || index < HLS_TRACK_VISIBLE_LIMIT || (track.uri != null && selectedUris.includes(track.uri)),
    );
  const hiddenCount = tracks.length - visibleTracks.length;

  return (
    <div className="flex flex-col gap-1 text-xs text-text-muted">
      <span>{heading}</span>
      <div className="flex flex-col gap-1">
        {visibleTracks.map(({ track, index }) => {
          const trackUri = track.uri ?? "";
          const disabled = !track.uri;
          const checked = selectedUris.includes(trackUri);
          const checkboxId = `${idPrefix}-${index}`;
          return (
            <label
              key={`${track.groupId}-${track.name}`}
              htmlFor={checkboxId}
              className={`flex items-center gap-2 ${disabled ? "opacity-50" : ""}`}
            >
              <Checkbox
                id={checkboxId}
                checked={checked}
                disabled={disabled}
                onChange={(e) => {
                  if (disabled) return;
                  onToggle((prev) => (e.target.checked ? [...prev, trackUri] : prev.filter((u) => u !== trackUri)));
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
      {/* Stay mounted while expanded so the list can be collapsed again. */}
      {showAll || hiddenCount > 0 ? (
        <button
          type="button"
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
          className="self-start text-[11px] text-accent-primary transition-colors hover:text-accent-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary"
        >
          {showAll ? t("newDownload.hlsFewerTracks") : t("newDownload.hlsShowMoreTracks", { n: hiddenCount })}
        </button>
      ) : null}
    </div>
  );
}

function SharedCreateDraftFields({
  priority,
  setPriority,
  categoryKey,
  setCategoryKey,
  speedAmount,
  setSpeedAmount,
  speedUnit,
  setSpeedUnit,
  proxyMode,
  setProxyMode,
  proxyUrl,
  setProxyUrl,
  proxyUsername,
  setProxyUsername,
  proxyPassword,
  setProxyPassword,
  proxyNoProxy,
  setProxyNoProxy,
}: {
  priority: TaskPriority;
  setPriority: (value: TaskPriority) => void;
  categoryKey: string;
  setCategoryKey: (value: string) => void;
  speedAmount: string;
  setSpeedAmount: (value: string) => void;
  speedUnit: string;
  setSpeedUnit: (value: string) => void;
  proxyMode: TaskProxyMode;
  setProxyMode: (value: TaskProxyMode) => void;
  proxyUrl: string;
  setProxyUrl: (value: string) => void;
  proxyUsername: string;
  setProxyUsername: (value: string) => void;
  proxyPassword: string;
  setProxyPassword: (value: string) => void;
  proxyNoProxy: string;
  setProxyNoProxy: (value: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-3 border-t border-border-subtle pt-3">
      <div className="grid grid-cols-2 gap-2">
        <div className="flex flex-col gap-1 text-xs text-text-muted">
          <span id="new-download-priority-label">{t("newDownload.priority")}</span>
          <Select value={priority} onValueChange={(value) => setPriority(value as TaskPriority)}>
            <SelectTrigger aria-labelledby="new-download-priority-label" className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="high">{t("taskDetails.priorityHigh")}</SelectItem>
              <SelectItem value="normal">{t("taskDetails.priorityNormal")}</SelectItem>
              <SelectItem value="low">{t("taskDetails.priorityLow")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <label htmlFor="new-download-category" className="flex flex-col gap-1 text-xs text-text-muted">
          {t("newDownload.category")}
          <Input
            id="new-download-category"
            value={categoryKey}
            onChange={(event) => setCategoryKey(event.target.value)}
            placeholder={t("newDownload.categoryPlaceholder")}
            className="h-8"
          />
        </label>
      </div>
      <div className="flex flex-col gap-1 text-xs text-text-muted">
        <span id="new-download-speed-limit-label">{t("newDownload.taskSpeedLimit")}</span>
        <div className="flex gap-2">
          <Input
            id="new-download-speed-limit"
            value={speedAmount}
            onChange={(event) => setSpeedAmount(event.target.value)}
            placeholder={t("speedLimit.unlimited")}
            className="h-8"
            inputMode="decimal"
            aria-labelledby="new-download-speed-limit-label"
          />
          <Select value={speedUnit} onValueChange={setSpeedUnit}>
            <SelectTrigger aria-labelledby="new-download-speed-limit-label" className="h-8 w-28 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SPEED_LIMIT_UNITS.map((unit) => (
                <SelectItem key={unit.value} value={unit.value}>
                  {speedLimitUnitLabel(unit.byteUnitKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-col gap-1 text-xs text-text-muted">
          <span id="new-download-proxy-mode-label">{t("settings.proxyMode")}</span>
          <Select value={proxyMode} onValueChange={(value) => setProxyMode(value as TaskProxyMode)}>
            <SelectTrigger aria-labelledby="new-download-proxy-mode-label" className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="inherit">{t("taskDetails.proxyInherit")}</SelectItem>
              <SelectItem value="off">{t("taskDetails.proxyOff")}</SelectItem>
              <SelectItem value="custom">{t("taskDetails.proxyCustom")}</SelectItem>
            </SelectContent>
          </Select>
        </div>
        {proxyMode === "custom" ? (
          <div className="grid gap-2">
            <Input
              value={proxyUrl}
              onChange={(event) => setProxyUrl(event.target.value)}
              placeholder={t("settings.proxyUrlPlaceholder")}
              className="h-8 font-mono text-xs"
              aria-label={t("settings.proxyUrl")}
            />
            <div className="grid grid-cols-2 gap-2">
              <Input
                value={proxyUsername}
                onChange={(event) => setProxyUsername(event.target.value)}
                placeholder={t("settings.proxyUsername")}
                className="h-8"
                aria-label={t("settings.proxyUsername")}
              />
              <Input
                type="password"
                value={proxyPassword}
                onChange={(event) => setProxyPassword(event.target.value)}
                placeholder={t("settings.proxyPassword")}
                className="h-8"
                aria-label={t("settings.proxyPassword")}
              />
            </div>
            <Input
              value={proxyNoProxy}
              onChange={(event) => setProxyNoProxy(event.target.value)}
              placeholder={t("settings.proxyNoProxyPlaceholder")}
              className="h-8 font-mono text-xs"
              aria-label={t("settings.proxyNoProxy")}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function removeCreatedBatchLines(input: string, result: BatchImportResult): string {
  let resultIndex = 0;
  return input
    .split(/\r?\n/)
    .filter((line) => {
      if (!line.trim()) return true;
      const item = result.items[resultIndex++];
      return !item?.task;
    })
    .join("\n");
}

function removeBatchUrls(input: string, urls: string[]): string {
  const remaining = new Map<string, number>();
  for (const url of urls) remaining.set(url, (remaining.get(url) ?? 0) + 1);
  return input
    .split(/\r?\n/)
    .filter((line) => {
      const trimmed = line.trim();
      if (!trimmed) return true;
      const count = remaining.get(trimmed) ?? 0;
      if (count === 0) return true;
      if (count === 1) remaining.delete(trimmed);
      else remaining.set(trimmed, count - 1);
      return false;
    })
    .join("\n");
}

function mergeBatchRetry(
  previous: BatchImportResult,
  indices: number[],
  retried: BatchImportResult,
): BatchImportResult {
  const items = [...previous.items];
  indices.forEach((originalIndex, retryIndex) => {
    if (retried.items[retryIndex]) items[originalIndex] = retried.items[retryIndex];
  });
  return {
    items,
    createdCount: items.filter((item) => item.task).length,
    failedCount: items.filter(isFailedBatchItem).length,
    duplicateCount: items.filter((item) => item.duplicate).length,
  };
}
