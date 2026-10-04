import * as Dialog from "@radix-ui/react-dialog";
import {
  ChevronDown,
  Clipboard,
  ClipboardCopy,
  File,
  FolderOpen,
  Hash,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import {
  Component,
  type ErrorInfo,
  type KeyboardEvent,
  memo,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { ChunkMap, ChunkMapSummary } from "@/components/shell/ChunkMap";
import { TaskPassportCard } from "@/components/shell/TaskPassportCard";
import { TaskTimeline } from "@/components/shell/TaskTimeline";
import { rowShowsRetry, rowTransferMode, torrentFileSelectionRequired } from "@/components/tasks/row-recovery";
import { SpeedSparkline } from "@/components/tasks/SpeedSparkline";
import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { MenuItem, MenuSeparator, RegionContextMenu } from "@/components/ui/menu-item";
import { ProgressBar } from "@/components/ui/progress-bar";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type {
  ChecksumAlgorithm,
  DashSegmentView,
  HashVerificationState,
  HlsSegmentView,
  MetalinkMirrorView,
  RecoveryAction,
  RequestDiagnostic,
  SegmentSummary,
  SftpKnownHost,
  TaskChecksum,
  TaskEvent,
  TaskNetworkPolicyView,
  TaskPriority,
  TaskProxyMode,
  TaskProxySettings,
  TorrentRuntimeSnapshot,
} from "@/generated/bindings";
import { useIsCompactShell } from "@/hooks/use-shell-layout";
import { useTaskDetailQueries } from "@/hooks/use-task-detail-queries";
import type { TranslationKey } from "@/i18n";
import { chunkMapCells, hasByteRangeSegments } from "@/lib/chunk-map";
import { writeClipboardText } from "@/lib/clipboard-write";
import { errorMessage, localizedErrorMessage, parseAppError } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import { timelinePayloadSummary } from "@/lib/integrity-passport";
import { createLogger } from "@/lib/logger";
import {
  SPEED_LIMIT_UNITS,
  speedLimitBytesFromInput,
  speedLimitInputFromBytes,
  speedLimitUnitLabel,
} from "@/lib/speed-limit";
import {
  capabilityChips,
  detailDiagnosis,
  diagnosticsRequestsEmptyKey,
  diagnosticsSegmentsEmptyKey,
  ftpTlsModeLabel,
  isDashProtocol,
  isHlsProtocol,
  isTorrentProtocol,
  parseUrlHostPort,
  showsHttpRequestFields,
  showsTransferRates,
} from "@/lib/task-diagnostics";
import {
  computeFileHash,
  finishLiveRecording,
  getTaskNetworkPolicy,
  getTaskProxySettings,
  listMetalinkMirrors,
  onTaskUpdated,
  retryTaskWithMirror,
  revokeTaskNetworkAuthorization,
  updateTaskProxySettings,
  updateTaskTransferOptions,
  updateTorrentFileSelection,
  updateTorrentSeeding,
  verifyTaskHash,
} from "@/lib/tauri";
import { cn, formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/utils";
import { type SpeedSample, useSpeedHistoryStore } from "@/stores/speed-history-store";
import { useTaskDataStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { TaskRequestProfilePanel } from "./TaskRequestProfilePanel";

const log = createLogger("task-details");

/** `ftpTlsModeLabel` returns a closed mode union, so this lookup is exhaustive. */
const FTP_TLS_MODE_KEYS = {
  plain: "taskDetails.ftpTls.plain",
  explicit: "taskDetails.ftpTls.explicit",
  implicit: "taskDetails.ftpTls.implicit",
} as const satisfies Record<NonNullable<ReturnType<typeof ftpTlsModeLabel>>, TranslationKey>;

const EMPTY_TASK_FILES: Task["files"] = [];
const EMPTY_SPEED_HISTORY: SpeedSample[] = [];

/** The same transfer actions the task row offers. The panel repeats them so a
 * user reading the details never has to go back to the row to act on them. */
export interface TaskDetailsActionHandlers {
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onRedownload?: (task: Task) => void;
  onRecheck?: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onDelete?: (task: Task) => void;
}

interface TaskDetailsProps {
  taskId: string | null;
  open: boolean;
  onClose?: () => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  actions?: TaskDetailsActionHandlers;
}

export function TaskDetails({ taskId, open, onClose, onResolveAttention, actions }: TaskDetailsProps) {
  // Subscribe to the task object directly from the store. This keeps the
  // per-tick re-render scoped to TaskDetails only, removing AppShell from
  // the progress-tick render path.
  const task = useTaskDataStore((s) => (taskId ? (s.taskById[taskId] ?? null) : null));
  const compact = useIsCompactShell();
  const [, setRefreshTick] = useState(0);
  const onRefresh = useCallback(() => setRefreshTick((prev) => prev + 1), []);
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);

  const copyToClipboard = useCallback(
    async (text: string, successKey: TranslationKey) => {
      try {
        await writeClipboardText(text);
        addToast({ tone: "success", title: t(successKey) });
      } catch {
        addToast({ tone: "error", title: t("contextmenu.task.copyFailed") });
      }
    },
    [addToast, t],
  );

  if (!open || !task) return null;

  if (compact) {
    return (
      <TaskDetailsDrawer
        task={task}
        open={open}
        onClose={onClose}
        onResolveAttention={onResolveAttention}
        onRefresh={onRefresh}
        actions={actions}
      />
    );
  }

  const items = (
    <>
      <MenuItem
        icon={Hash}
        label={t("contextmenu.task.copyId")}
        onSelect={() => void copyToClipboard(task.id, "contextmenu.task.idCopied")}
      />
      <MenuItem
        icon={ClipboardCopy}
        label={t("contextmenu.task.copyUrl")}
        onSelect={() => void copyToClipboard(task.url, "contextmenu.task.urlCopied")}
      />
      <MenuItem
        icon={Clipboard}
        label={t("contextmenu.task.copyLocalPath")}
        onSelect={() =>
          void copyToClipboard(task.finalPath ?? `${task.saveDir}/${task.fileName}`, "contextmenu.task.pathCopied")
        }
      />
      <MenuSeparator />
      <MenuItem icon={RefreshCw} label={t("taskDetails.refresh")} onSelect={onRefresh} />
      {onClose && (
        <>
          <MenuSeparator />
          <MenuItem icon={X} label={t("taskDetails.close")} onSelect={onClose} />
        </>
      )}
    </>
  );

  return (
    <RegionContextMenu items={items}>
      <aside
        className={cn(
          "flex w-80 shrink-0 flex-col border-l border-border-subtle bg-surface-base xl:w-96",
          "motion-safe:animate-[detail-enter_220ms_cubic-bezier(0.16,1,0.3,1)_both]",
        )}
        aria-labelledby="task-details-heading"
        onKeyDown={(event) => closeOnEscape(event, onClose)}
      >
        <TaskDetailsHeader task={task} onClose={onClose} actions={actions} />
        <TaskDetailsErrorBoundary taskId={task.id} onClose={onClose}>
          <TaskDetailsPanel task={task} onResolveAttention={onResolveAttention} onRefresh={onRefresh} />
        </TaskDetailsErrorBoundary>
      </aside>
    </RegionContextMenu>
  );
}

class TaskDetailsErrorBoundary extends Component<
  {
    taskId: string;
    children: ReactNode;
    onClose?: () => void;
  },
  { hasError: boolean }
> {
  state = { hasError: false };

  static getDerivedStateFromError() {
    return { hasError: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    log.warn("task details render failed", { error, componentStack: info.componentStack });
  }

  componentDidUpdate(prevProps: { taskId: string }) {
    if (prevProps.taskId !== this.props.taskId && this.state.hasError) {
      this.setState({ hasError: false });
    }
  }

  render() {
    if (this.state.hasError) {
      return (
        <TaskDetailsErrorFallback onClose={this.props.onClose} onRetry={() => this.setState({ hasError: false })} />
      );
    }

    return this.props.children;
  }
}

function TaskDetailsErrorFallback({ onClose, onRetry }: { onClose?: () => void; onRetry: () => void }) {
  const { t } = useTranslation();

  return (
    <div className="flex min-h-0 flex-1 flex-col justify-center gap-3 px-4 py-6">
      <div className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-sm" role="alert">
        <p className="font-medium text-status-danger">{t("taskDetails.detailsUnavailable")}</p>
        <p className="mt-1 text-xs leading-5 text-text-secondary">{t("taskDetails.detailsUnavailableDescription")}</p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" size="sm" variant="outline" onClick={onRetry}>
          {t("taskDetails.retryDetails")}
        </Button>
        {onClose ? (
          <Button type="button" size="sm" variant="ghost" onClick={onClose}>
            {t("taskDetails.close")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Esc closes the docked panel the way it closes the drawer and every dialog.
 * Only keys pressed inside the panel's own DOM count: selects and menus portal
 * their popups out of it (React still bubbles their events here), and they use
 * Esc to close themselves first.
 */
function closeOnEscape(event: KeyboardEvent<HTMLElement>, onClose?: () => void) {
  if (event.key !== "Escape" || event.defaultPrevented || !onClose) return;
  if (!(event.target instanceof Node) || !event.currentTarget.contains(event.target)) return;
  event.preventDefault();
  onClose();
}

function TaskDetailsActions({ task, actions }: { task: Task; actions?: TaskDetailsActionHandlers }) {
  const { t } = useTranslation();
  if (!actions) return null;
  // Same rules as the row: a failed task with a recovery playbook offers its
  // fixes in the overview instead of a Resume/Retry that would contradict them.
  const transferMode = rowTransferMode(task);
  const showRetry = rowShowsRetry(task);
  const buttonClass = "h-8 gap-1.5 px-2.5 text-xs";

  return (
    <fieldset className="m-0 mt-2 flex min-w-0 flex-wrap gap-1.5 border-0 p-0">
      <legend className="sr-only">{t("taskDetails.actionsAria")}</legend>
      {transferMode !== "hidden" ? (
        <Button
          type="button"
          variant="outline"
          size="sm"
          className={buttonClass}
          onClick={() => actions.onToggleTransfer(task)}
        >
          {transferMode === "resume" ? (
            <Play className="h-3.5 w-3.5" aria-hidden />
          ) : (
            <Pause className="h-3.5 w-3.5" aria-hidden />
          )}
          {transferMode === "resume" ? t("actions.resume") : t("actions.pause")}
        </Button>
      ) : null}
      {showRetry ? (
        <Button type="button" variant="outline" size="sm" className={buttonClass} onClick={() => actions.onRetry(task)}>
          <RotateCcw className="h-3.5 w-3.5" aria-hidden />
          {t("actions.retry")}
        </Button>
      ) : null}
      {task.status === "completed" ? (
        <>
          {actions.onRedownload ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className={buttonClass}
              onClick={() => actions.onRedownload?.(task)}
            >
              <RotateCcw className="h-3.5 w-3.5" aria-hidden />
              {t("actions.redownload")}
            </Button>
          ) : null}
          {actions.onRecheck ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className={buttonClass}
              onClick={() => actions.onRecheck?.(task)}
            >
              <Hash className="h-3.5 w-3.5" aria-hidden />
              {t("actions.recheck")}
            </Button>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            className={buttonClass}
            onClick={() => actions.onOpenFile(task)}
          >
            <File className="h-3.5 w-3.5" aria-hidden />
            {t("actions.openFile")}
          </Button>
        </>
      ) : null}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className={buttonClass}
        onClick={() => actions.onOpenFolder(task)}
      >
        <FolderOpen className="h-3.5 w-3.5" aria-hidden />
        {t("actions.openFolder")}
      </Button>
      {actions.onDelete ? (
        // Delete here is the undoable soft delete, so it sits apart at the end
        // and turns red only on hover or focus instead of outweighing Pause.
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={cn(
            buttonClass,
            "ml-auto text-text-muted hover:bg-status-danger/10 hover:text-status-danger focus-visible:text-status-danger",
          )}
          onClick={() => actions.onDelete?.(task)}
        >
          <Trash2 className="h-3.5 w-3.5" aria-hidden />
          {t("deleteDialog.confirm")}
        </Button>
      ) : null}
    </fieldset>
  );
}

function TaskDetailsDrawer({
  task,
  open,
  onClose,
  onResolveAttention,
  onRefresh,
  actions,
}: {
  task: Task;
  open: boolean;
  onClose?: () => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onRefresh: () => void;
  actions?: TaskDetailsActionHandlers;
}) {
  const { t } = useTranslation();

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose?.();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-x-0 bottom-0 top-[var(--titlebar-height)] z-40 bg-surface-scrim motion-safe:animate-[fade-in_180ms_ease-out]" />
        {/* Keep window chrome reachable on Windows/macOS custom titlebars (Linux token is 0). */}
        <Dialog.Content
          className={cn(
            "fixed right-0 bottom-0 top-[var(--titlebar-height)] z-50 flex h-[calc(100dvh-var(--titlebar-height))] w-full max-w-sm flex-col border-l border-border-subtle bg-surface-base shadow-xl",
            "motion-safe:animate-[drawer-enter_220ms_cubic-bezier(0.16,1,0.3,1)_both]",
            "focus:outline-none",
          )}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            const target = event.currentTarget;
            if (!(target instanceof HTMLElement)) return;
            target.querySelector<HTMLElement>("[data-task-details-close]")?.focus();
          }}
        >
          <header className="flex shrink-0 items-start gap-2 border-b border-border-subtle px-4 py-3">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="truncate text-sm font-medium" title={task.fileName}>
                {task.fileName}
              </Dialog.Title>
              <p className="truncate text-xs text-text-muted" title={task.saveDir}>
                {task.saveDir}
              </p>
              <TaskDetailsStatus task={task} />
              <TaskDetailsActions task={task} actions={actions} />
            </div>
            <Dialog.Close asChild>
              <Button
                variant="ghost"
                size="icon"
                className="h-10 w-10 shrink-0"
                aria-label={t("taskDetails.close")}
                data-task-details-close
              >
                <X className="h-4 w-4" />
              </Button>
            </Dialog.Close>
          </header>
          <Dialog.Description className="sr-only">
            {t("taskDetails.drawerDescription", { name: task.fileName })}
          </Dialog.Description>
          <TaskDetailsErrorBoundary taskId={task.id} onClose={onClose}>
            <TaskDetailsPanel task={task} onResolveAttention={onResolveAttention} onRefresh={onRefresh} />
          </TaskDetailsErrorBoundary>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function TaskDetailsHeader({
  task,
  onClose,
  actions,
}: {
  task: Task;
  onClose?: () => void;
  actions?: TaskDetailsActionHandlers;
}) {
  const { t } = useTranslation();

  return (
    <header className="flex shrink-0 items-start gap-2 border-b border-border-subtle px-4 py-3">
      <div className="min-w-0 flex-1">
        <h2 id="task-details-heading" className="truncate text-sm font-medium" title={task.fileName}>
          {task.fileName}
        </h2>
        <p className="truncate text-xs text-text-muted" title={task.saveDir}>
          {task.saveDir}
        </p>
        <TaskDetailsStatus task={task} />
        <TaskDetailsActions task={task} actions={actions} />
      </div>
      {onClose ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-10 w-10 shrink-0"
              aria-label={t("taskDetails.close")}
              data-task-details-close
              onClick={onClose}
            >
              <X className="h-4 w-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {t("taskDetails.close")}
            <kbd className="ml-2 font-mono text-[11px] opacity-70">Esc</kbd>
          </TooltipContent>
        </Tooltip>
      ) : null}
    </header>
  );
}

function TaskDetailsStatus({ task }: { task: Task }) {
  const { t } = useTranslation();
  const tone =
    task.status === "failed"
      ? "text-status-danger"
      : task.status === "needs_attention"
        ? "text-status-warning"
        : "text-text-secondary";
  return (
    <span
      className={cn(
        "mt-1 inline-flex rounded-full border border-border-subtle px-1.5 py-0.5 text-xs font-medium",
        tone,
      )}
    >
      {t(`task.status.${task.status}`)}
    </span>
  );
}

function TaskDetailsPanel({
  task,
  onResolveAttention,
  onRefresh,
}: {
  task: Task;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState("overview");
  const speedHistory = useSpeedHistoryStore((s) => s.history[task.id] ?? EMPTY_SPEED_HISTORY);
  const [hashState, setHashState] = useState<HashVerificationState | null>(null);
  const [verifyingHash, setVerifyingHash] = useState(false);
  const [checksumResults, setChecksumResults] = useState<Record<string, { actual: string; match: boolean }>>({});
  const [verifyingChecksums, setVerifyingChecksums] = useState<Set<string>>(new Set());
  const [mirrors, setMirrors] = useState<MetalinkMirrorView[]>([]);
  const [finishingLive, setFinishingLive] = useState(false);
  const isMetalinkTask = task.protocol === "metalink";
  const isTorrentTask = isTorrentProtocol(task.protocol);
  const isHlsTask = isHlsProtocol(task.protocol);
  const isDashTask = isDashProtocol(task.protocol);
  const canFinishLiveRecording = isHlsTask && (task.status === "downloading" || task.status === "retrying");

  const {
    segments,
    segmentsCursor,
    segmentError,
    hlsSegments,
    hlsSegmentsCursor,
    hlsSegmentError,
    dashSegments,
    dashSegmentsCursor,
    dashSegmentError,
    events,
    eventsCursor,
    eventsError,
    requests,
    requestsCursor,
    requestsError,
    torrentSnapshot,
    torrentSnapshotError,
    segmentSummary,
    segmentSummaryError,
    ftpSftpEvents,
    sftpKnownHosts,
    passport,
    passportError,
    loadMoreSegments,
    loadMoreHlsSegments,
    loadMoreDashSegments,
    loadMoreEvents,
    loadMoreRequests,
  } = useTaskDetailQueries({ task, activeTab });

  // biome-ignore lint/correctness/useExhaustiveDependencies: task identity resets the panel; later status updates must preserve the selected tab.
  useEffect(() => {
    // Recovery is the time-sensitive next step for failed tasks, and it lives
    // in Overview. Starting there makes the corrective action available before
    // the user has to discover and leave a diagnostics-only tab.
    setActiveTab("overview");
    setHashState(null);
    setChecksumResults({});
    setVerifyingChecksums(new Set());
    setMirrors([]);
    setFinishingLive(false);
  }, [task.id]);

  // Event-driven refresh: re-fetch detail data when this task's status/metadata changes,
  // instead of relying solely on polling. Uses a tick counter that polling effects
  // depend on, so they re-run immediately when an event arrives.
  // Debounced 300ms to avoid flooding when many progress events arrive in quick succession.
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void onTaskUpdated((updatedTask) => {
      if (cancelled) return;
      if (updatedTask.id === task.id) {
        if (refreshTimer.current) clearTimeout(refreshTimer.current);
        refreshTimer.current = setTimeout(() => {
          onRefresh();
        }, 300);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      unlisten?.();
    };
  }, [task.id, onRefresh]);

  // Load Metalink mirrors
  useEffect(() => {
    if (!isMetalinkTask || activeTab !== "overview") {
      setMirrors([]);
      return;
    }
    let cancelled = false;
    void listMetalinkMirrors(task.id)
      .then((data) => {
        if (!cancelled) setMirrors(data);
      })
      .catch((err) => {
        log.warn("failed to load metalink mirrors", err);
      });
    return () => {
      cancelled = true;
    };
  }, [isMetalinkTask, task.id, activeTab]);

  async function handleRetryWithMirror(mirrorUrl: string) {
    try {
      await retryTaskWithMirror(task.id, mirrorUrl);
    } catch (err) {
      log.warn("retry with mirror failed", err);
    }
  }

  async function handleFinishLiveRecording() {
    setFinishingLive(true);
    try {
      await finishLiveRecording(task.id);
    } catch (err) {
      log.warn("finish live recording failed", err);
    } finally {
      setFinishingLive(false);
    }
  }

  async function runHashVerification() {
    setVerifyingHash(true);
    try {
      setHashState(await verifyTaskHash(task.id));
    } catch (err) {
      log.warn("hash verification failed", err);
    } finally {
      setVerifyingHash(false);
    }
  }

  async function verifyChecksum(algo: ChecksumAlgorithm) {
    setVerifyingChecksums((prev) => new Set(prev).add(algo));
    try {
      const actual = await computeFileHash(task.id, algo);
      const checksum = task.checksums.find((cs) => cs.algorithm === algo);
      const match = checksum ? actual.toLowerCase() === checksum.expectedHash.toLowerCase() : false;
      setChecksumResults((prev) => ({ ...prev, [algo]: { actual, match } }));
    } catch (err) {
      log.warn("checksum verification failed", err);
    } finally {
      setVerifyingChecksums((prev) => {
        const next = new Set(prev);
        next.delete(algo);
        return next;
      });
    }
  }

  function verifyAllChecksums() {
    for (const cs of task.checksums) {
      void verifyChecksum(cs.algorithm);
    }
  }

  const isFailedOrAttention = task.status === "failed" || task.status === "needs_attention";
  const isCompleted = task.status === "completed";
  const chunkCells = useMemo(
    () => (hasByteRangeSegments(task.protocol) ? chunkMapCells(segments, task.totalSize, task.status) : null),
    [segments, task.protocol, task.totalSize, task.status],
  );
  const overallPercent =
    task.totalSize > 0 ? Math.min(100, Math.max(0, Math.round((task.downloadedBytes / task.totalSize) * 100))) : 0;

  const hashPanel = (
    <HashPanel
      task={task}
      state={hashState}
      verifying={verifyingHash}
      onVerify={() => void runHashVerification()}
      checksumResults={checksumResults}
      verifyingChecksums={verifyingChecksums}
      onVerifyChecksum={(algo) => void verifyChecksum(algo)}
      onVerifyAllChecksums={verifyAllChecksums}
    />
  );
  const recoveryActions = <TaskRecoveryActions task={task} onResolve={onResolveAttention} />;

  return (
    <Tabs value={activeTab} onValueChange={setActiveTab} className="flex min-h-0 flex-1 flex-col px-4 py-3">
      <TabsList className="w-full justify-start overflow-x-auto">
        <TabsTrigger value="overview">{t("taskDetails.overview")}</TabsTrigger>
        <TabsTrigger value="diagnostics">{t("taskDetails.diagnostics")}</TabsTrigger>
        <TabsTrigger value="logs">{t("taskDetails.logs")}</TabsTrigger>
      </TabsList>

      <ScrollArea className="min-h-0 flex-1">
        {/* One scroll with sections instead of tabs inside a tab with a mode
            toggle inside that: the chunk ranges were three clicks deep. */}
        <TabsContent value="diagnostics" className="space-y-5">
          {!isTorrentTask ? (
            <DiagnosticsSection id="panel-segments" title={t("taskDetails.segments")}>
              {isHlsTask ? (
                <HlsSegmentList
                  segments={hlsSegments}
                  error={hlsSegmentError}
                  emptyLabel={t(diagnosticsSegmentsEmptyKey(task.protocol))}
                  sequenceLabel={t("taskDetails.hlsSequence")}
                  statusLabel={t("taskDetails.hlsStatus")}
                  durationLabel={t("taskDetails.hlsDuration")}
                  retriesLabel={t("taskDetails.chunkRetries")}
                  hasMore={Boolean(hlsSegmentsCursor)}
                  loadMoreLabel={t("taskDetails.loadMore")}
                  onLoadMore={loadMoreHlsSegments}
                />
              ) : isDashTask ? (
                <DashSegmentList
                  segments={dashSegments}
                  error={dashSegmentError}
                  emptyLabel={t(diagnosticsSegmentsEmptyKey(task.protocol))}
                  trackLabel={t("taskDetails.dashTrack")}
                  indexLabel={t("taskDetails.dashIndex")}
                  statusLabel={t("taskDetails.dashStatus")}
                  retriesLabel={t("taskDetails.chunkRetries")}
                  hasMore={Boolean(dashSegmentsCursor)}
                  loadMoreLabel={t("taskDetails.loadMore")}
                  onLoadMore={loadMoreDashSegments}
                />
              ) : (
                <SegmentList
                  segments={segments}
                  taskSpeedBps={task.speedBps}
                  error={segmentError}
                  emptyLabel={t(diagnosticsSegmentsEmptyKey(task.protocol))}
                  hasMore={Boolean(segmentsCursor)}
                  loadMoreLabel={t("taskDetails.loadMore")}
                  onLoadMore={loadMoreSegments}
                />
              )}
            </DiagnosticsSection>
          ) : null}
          <DiagnosticsSection id="panel-requests" title={t("taskDetails.requests")}>
            <RequestList
              requests={requests}
              error={requestsError}
              emptyLabel={t(diagnosticsRequestsEmptyKey(task.protocol))}
              hasMore={Boolean(requestsCursor)}
              loadMoreLabel={t("taskDetails.loadMore")}
              onLoadMore={loadMoreRequests}
            />
          </DiagnosticsSection>
        </TabsContent>
        <TabsContent value="overview" className="space-y-3 text-sm">
          <div className="space-y-2">
            <OverviewDiagnosis task={task} />
            {chunkCells ? (
              <ChunkMap cells={chunkCells} segments={segments} percent={overallPercent} taskStatus={task.status} />
            ) : (
              <ProgressBar
                value={task.totalSize > 0 ? task.downloadedBytes / task.totalSize : 0}
                label={formatPercent(task.downloadedBytes, task.totalSize)}
                active={task.status === "downloading" || task.status === "retrying"}
                size="lg"
                tone={task.status === "completed" ? "success" : task.status === "failed" ? "danger" : "primary"}
              />
            )}
            <div className="flex items-center justify-between px-1">
              <span className="text-xs text-text-muted">{formatPercent(task.downloadedBytes, task.totalSize)}</span>
              <span className="font-mono text-xs text-text-muted">
                {formatBytes(task.downloadedBytes)} / {formatBytes(task.totalSize)}
              </span>
            </div>
            {chunkCells ? (
              <ChunkMapSummary
                segments={segments}
                taskStatus={task.status}
                onViewRanges={() => setActiveTab("diagnostics")}
              />
            ) : null}
            <CapabilityChips task={task} />
          </div>
          {showsTransferRates(task.status) ? (
            <div className="space-y-0.5">
              <Row label={t("taskDetails.speed")} value={formatSpeed(task.speedBps, { fixed: true })} />
              <Row
                label={t("taskDetails.eta")}
                value={formatEta(task.downloadedBytes, task.totalSize, task.speedBps)}
              />
              {/* Speed history moved here from the row's in-place expansion,
                  which was the only other place it could be seen. */}
              <SpeedSparkline
                samples={speedHistory}
                currentSpeedBps={task.speedBps}
                label={t("task.expanded.speedHistoryAria", { name: task.fileName })}
                className="mt-1.5"
              />
            </div>
          ) : null}
          {canFinishLiveRecording ? (
            <div className="flex justify-end">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={finishingLive}
                onClick={() => void handleFinishLiveRecording()}
              >
                {finishingLive ? t("taskDetails.finishingRecording") : t("actions.finishRecording")}
              </Button>
            </div>
          ) : null}
          {/* Status-priority panel: failed → recovery first; completed → hash first */}
          {isFailedOrAttention ? recoveryActions : null}
          {isCompleted && !isFailedOrAttention ? hashPanel : null}
          <TaskPassportCard task={task} passport={passport} error={passportError} />
          {/* Protocol runtime panels */}
          <TorrentRuntimePanel task={task} snapshot={torrentSnapshot} error={torrentSnapshotError} />
          <MetalinkFilesPanel task={task} />
          <MetalinkMirrorPanel
            mirrors={mirrors}
            taskStatus={task.status}
            onRetryMirror={(url) => void handleRetryWithMirror(url)}
          />
          <FtpSftpOverviewPanel
            task={task}
            summary={segmentSummary}
            summaryError={segmentSummaryError}
            events={ftpSftpEvents}
            knownHosts={sftpKnownHosts}
          />
          {/* Non-priority hash/recovery */}
          {!isCompleted || isFailedOrAttention ? hashPanel : null}
          {!isFailedOrAttention ? recoveryActions : null}
          <TaskTimeline events={events} error={eventsError} onOpenLogs={() => setActiveTab("logs")} />
          {/* Advanced settings (collapsed by default to reduce cognitive load) */}
          <AdvancedSettingsDisclosure task={task} />
        </TabsContent>
        <TabsContent value="logs">
          <EventList
            events={events}
            error={eventsError}
            emptyLabel={t("taskDetails.noLogs")}
            hasMore={Boolean(eventsCursor)}
            loadMoreLabel={t("taskDetails.loadMore")}
            onLoadMore={loadMoreEvents}
          />
        </TabsContent>
      </ScrollArea>
    </Tabs>
  );
}

/** The engine's verdict above the progress strip, so the first line of the
 * overview says why the numbers below look the way they do. */
function OverviewDiagnosis({ task }: { task: Task }) {
  const { t } = useTranslation();
  const diagnosis = detailDiagnosis(task, t);
  if (!diagnosis) return null;
  return <p className="px-1 text-sm font-medium leading-5 text-text-primary">{diagnosis}</p>;
}

function CapabilityChips({ task }: { task: Task }) {
  const { t } = useTranslation();
  const chips = capabilityChips(task);
  if (chips.length === 0) return null;
  return (
    <ul aria-label={t("taskDetails.capabilitiesAria")} className="flex flex-wrap gap-1.5 px-1">
      {chips.map((chip) => (
        <li
          key={chip.labelKey}
          className={cn(
            "rounded-full border px-2 py-0.5 text-[11px] font-medium leading-4",
            chip.tone === "warning"
              ? "border-border-warning-subtle bg-status-warning/10 text-status-warning"
              : "border-border-subtle text-text-secondary",
          )}
        >
          {t(chip.labelKey)}
        </li>
      ))}
    </ul>
  );
}

function Row({ label, value, mono = true, hint }: { label: string; value: string; mono?: boolean; hint?: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3 rounded-md bg-surface-root/50 px-3 py-2">
      {hint ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="cursor-help rounded-sm text-xs text-text-muted underline decoration-dotted decoration-border-subtle underline-offset-2">
              {label}
            </span>
          </TooltipTrigger>
          <TooltipContent className="max-w-64 text-balance">{hint}</TooltipContent>
        </Tooltip>
      ) : (
        <div className="text-xs text-text-muted">{label}</div>
      )}
      <div className={cn("text-sm font-semibold text-text-primary", mono && "font-mono tabular-nums")}>{value}</div>
    </div>
  );
}

function formatAlgorithm(algo: string): string {
  switch (algo) {
    case "sha256":
      return "SHA-256";
    case "sha512":
      return "SHA-512";
    case "sha1":
      return "SHA-1";
    case "md5":
      return "MD5";
    default:
      return algo.toUpperCase();
  }
}

function ChecksumRow({
  checksum,
  result,
  verifying,
  disabled,
  onVerify,
}: {
  checksum: TaskChecksum;
  result: { actual: string; match: boolean } | undefined;
  verifying: boolean;
  disabled: boolean;
  onVerify: () => void;
}) {
  const { t } = useTranslation();
  const actual = result?.actual ?? checksum.actualHash;
  const match =
    result !== undefined
      ? result.match
      : checksum.status === "verified"
        ? true
        : checksum.status === "failed"
          ? false
          : null;

  return (
    <div className="py-1.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="font-semibold text-text-primary">{formatAlgorithm(checksum.algorithm)}</span>
          {checksum.weak ? (
            <span className="rounded bg-status-warning/15 px-1.5 py-0.5 text-[10px] text-status-warning">
              {t("taskDetails.weakAlgorithm")}
            </span>
          ) : null}
        </div>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="px-2 text-[11px]"
          onClick={onVerify}
          disabled={verifying || disabled}
        >
          {verifying ? "…" : t("taskDetails.verifyHash")}
        </Button>
      </div>
      <div className="mt-1 font-mono text-[11px] text-text-secondary">
        <div className="truncate" title={checksum.expectedHash}>
          <span className="text-text-muted">{t("taskDetails.expectedHash")} </span>
          {checksum.expectedHash}
        </div>
        {actual ? (
          <div className="truncate" title={actual}>
            <span className="text-text-muted">{t("taskDetails.actualHash")} </span>
            {actual}
          </div>
        ) : null}
      </div>
      {match !== null ? (
        <p
          role="status"
          className={cn("mt-1 text-[11px] font-medium", match ? "text-status-success" : "text-status-danger")}
        >
          {match ? t("taskDetails.checksumMatch") : t("taskDetails.checksumMismatch")}
        </p>
      ) : null}
      {checksum.errorMessage ? (
        <p role="alert" className="mt-1 text-[11px] text-status-danger">
          {checksum.errorMessage}
        </p>
      ) : null}
    </div>
  );
}

function HashPanel({
  task,
  state,
  verifying,
  onVerify,
  checksumResults,
  verifyingChecksums,
  onVerifyChecksum,
  onVerifyAllChecksums,
}: {
  task: Task;
  state: HashVerificationState | null;
  verifying: boolean;
  onVerify: () => void;
  checksumResults: Record<string, { actual: string; match: boolean }>;
  verifyingChecksums: Set<string>;
  onVerifyChecksum: (algo: ChecksumAlgorithm) => void;
  onVerifyAllChecksums: () => void;
}) {
  const { t } = useTranslation();
  const checksums = task.checksums ?? [];
  const isCompleted = task.status === "completed";
  const anyVerifying = verifyingChecksums.size > 0;

  if (checksums.length > 0) {
    return (
      <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
        <div className="flex items-center justify-between gap-3">
          <span className="text-text-muted">{t("taskDetails.checksumsHeader", { count: checksums.length })}</span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-8 shrink-0"
            onClick={onVerifyAllChecksums}
            disabled={anyVerifying || !isCompleted}
          >
            {anyVerifying ? t("taskDetails.verifyingAll") : t("taskDetails.verifyAll")}
          </Button>
        </div>
        <div className="mt-2 grid gap-2">
          {checksums.map((cs) => (
            <ChecksumRow
              key={`${cs.algorithm}-${cs.id}`}
              checksum={cs}
              result={checksumResults[cs.algorithm]}
              verifying={verifyingChecksums.has(cs.algorithm)}
              disabled={!isCompleted}
              onVerify={() => onVerifyChecksum(cs.algorithm)}
            />
          ))}
        </div>
      </div>
    );
  }

  // Legacy SHA-256 fallback
  const status = state?.status ?? task.hashStatus;
  const actual = state?.actualSha256 ?? task.actualHashSha256;
  const error = state?.errorMessage ?? task.hashError;

  if (!task.expectedHashSha256 && !state?.expectedSha256) {
    return <Row label={t("taskDetails.integrity")} value={t("taskDetails.hashNotRequested")} mono={false} />;
  }

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-text-muted">{t("taskDetails.integrity")}</div>
          <div className={cn("font-medium", hashTone(status))}>{t(`hash.status.${status}`)}</div>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-8 shrink-0"
          onClick={onVerify}
          disabled={verifying || !isCompleted}
        >
          {verifying ? t("taskDetails.verifyingHash") : t("taskDetails.verifyHash")}
        </Button>
      </div>
      <div className="mt-2 grid gap-1 font-mono text-[11px] text-text-secondary">
        <span className="break-all">
          {t("taskDetails.expectedHash")} {task.expectedHashSha256 ?? state?.expectedSha256}
        </span>
        {actual ? (
          <span className="break-all">
            {t("taskDetails.actualHash")} {actual}
          </span>
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="mt-2 text-status-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function mirrorTone(status: string): string {
  switch (status) {
    case "completed":
      return "text-status-success";
    case "failed":
      return "text-status-danger";
    default:
      return "text-text-secondary";
  }
}

function MetalinkFilesPanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  if (task.protocol !== "metalink") return null;
  const files = Array.isArray(task.files) ? task.files : EMPTY_TASK_FILES;
  if (files.length === 0) return null;

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="text-text-muted">{t("taskDetails.metalinkFilesHeader", { count: files.length })}</div>
      <div className="mt-2 grid gap-2">
        {files.map((file) => {
          const total = file.totalSize > 0 ? file.totalSize : 0;
          const progress = total > 0 ? file.downloadedBytes / total : 0;
          return (
            <div key={file.id} className="border-t border-border-divider py-2 first:border-t-0 first:pt-0">
              <div className="flex items-start justify-between gap-2">
                <div
                  className="min-w-0 flex-1 truncate font-mono text-[11px] text-text-secondary"
                  title={file.relativePath}
                >
                  {file.relativePath || file.fileName}
                </div>
                <span className="shrink-0 text-[11px] text-text-muted">{t(`task.status.${file.status}`)}</span>
              </div>
              <ProgressBar
                value={progress}
                label={formatPercent(file.downloadedBytes, total)}
                active={file.status === "downloading" || file.status === "retrying"}
                className="mt-1.5"
              />
              <div className="mt-1 flex justify-between font-mono text-[10px] text-text-muted">
                <span>{formatPercent(file.downloadedBytes, total)}</span>
                <span>
                  {formatBytes(file.downloadedBytes)} / {formatBytes(total)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function MetalinkMirrorPanel({
  mirrors,
  taskStatus,
  onRetryMirror,
}: {
  mirrors: MetalinkMirrorView[];
  taskStatus: string;
  onRetryMirror: (url: string) => void;
}) {
  const { t } = useTranslation();

  if (mirrors.length === 0) return null;

  const canRetry = taskStatus === "failed" || taskStatus === "needs_attention";

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="text-text-muted">{t("taskDetails.mirrorsHeader", { count: mirrors.length })}</div>
      <div className="mt-2 grid gap-2">
        {mirrors.map((mirror) => (
          <div
            key={mirror.id}
            className="flex items-start justify-between gap-2 border-t border-border-divider py-2 first:border-t-0 first:pt-0"
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className={cn("text-[11px] font-medium", mirrorTone(mirror.status))}>{mirror.status}</span>
                {mirror.location ? (
                  <span className="text-[10px] text-text-muted">
                    {t("taskDetails.mirrorLocation")}: {mirror.location}
                  </span>
                ) : null}
                {mirror.failureCount > 0 ? (
                  <span className="text-[10px] text-status-danger">
                    {t("taskDetails.mirrorFailures", { count: mirror.failureCount })}
                  </span>
                ) : null}
              </div>
              <div className="mt-0.5 truncate font-mono text-[11px] text-text-secondary" title={mirror.url}>
                {mirror.url}
              </div>
              {mirror.lastError ? <p className="mt-0.5 text-[10px] text-status-danger">{mirror.lastError}</p> : null}
            </div>
            {canRetry ? (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className="shrink-0 px-2 text-[10px]"
                onClick={() => onRetryMirror(mirror.url)}
              >
                {t("taskDetails.mirrorRetry")}
              </Button>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}

function FtpSftpOverviewPanel({
  task,
  summary,
  summaryError,
  events,
  knownHosts,
}: {
  task: Task;
  summary: SegmentSummary | null;
  summaryError: string | null;
  events: TaskEvent[];
  knownHosts: SftpKnownHost[];
}) {
  const { t } = useTranslation();
  if (task.protocol !== "ftp" && task.protocol !== "ftps" && task.protocol !== "sftp") return null;

  const tlsMode = ftpTlsModeLabel(task.protocol, task.url);
  const accelerationDisabled = events.some(
    (event) => event.eventType === "ftp_acceleration_disabled" || event.eventType === "sftp_acceleration_disabled",
  );
  const defaultPort = task.protocol === "sftp" ? 22 : task.protocol === "ftps" ? 990 : 21;
  const hostPort = parseUrlHostPort(task.url, defaultPort);
  const matchedHost =
    task.protocol === "sftp" && hostPort
      ? knownHosts.find(
          (host) => host.host.toLowerCase() === hostPort.host.toLowerCase() && host.port === hostPort.port,
        )
      : undefined;

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="text-text-muted">{t("taskDetails.ftpSftpRuntime")}</div>
      <div className="mt-2 space-y-0.5">
        <Row label={t("taskDetails.protocol")} value={task.protocol.toUpperCase()} />
        {tlsMode ? <Row label={t("taskDetails.ftpTlsMode")} value={t(FTP_TLS_MODE_KEYS[tlsMode])} /> : null}
        <Row label={t("taskDetails.connections")} value={String(task.connectionCount)} />
        {summaryError ? (
          <p
            role="alert"
            className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-status-danger"
          >
            {summaryError}
          </p>
        ) : summary ? (
          <Row
            label={t("taskDetails.segmentSummaryLabel")}
            value={t("taskDetails.ftpSegmentSummary", {
              completed: summary.completed,
              total: summary.total,
              active: summary.active,
              failed: summary.failed,
            })}
          />
        ) : null}
        <Row
          label={t("taskDetails.acceleration")}
          value={accelerationDisabled ? t("taskDetails.accelerationDisabled") : t("taskDetails.accelerationAvailable")}
        />
        {task.protocol === "sftp" ? (
          <Row
            label={t("taskDetails.sftpHostKey")}
            value={
              matchedHost
                ? `${matchedHost.algorithm} ${matchedHost.fingerprintSha256}`
                : t("taskDetails.sftpHostKeyUnknown")
            }
            mono={Boolean(matchedHost)}
            hint={t("taskDetails.sftpHostKeyHint")}
          />
        ) : null}
      </div>
    </div>
  );
}

function TorrentRuntimePanel({
  task,
  snapshot,
  error,
}: {
  task: Task;
  snapshot: TorrentRuntimeSnapshot | null;
  error: string | null;
}) {
  const { t } = useTranslation();
  const taskFiles = Array.isArray(task.files) ? task.files : EMPTY_TASK_FILES;
  const [selectedFiles, setSelectedFiles] = useState<Set<string>>(
    () => new Set(taskFiles.filter((file) => file.selected).map((file) => file.relativePath)),
  );
  const [saving, setSaving] = useState(false);
  const [ratioLimitDraft, setRatioLimitDraft] = useState("");
  const [timeLimitDraft, setTimeLimitDraft] = useState("");
  useEffect(() => {
    setSelectedFiles(new Set(taskFiles.filter((file) => file.selected).map((file) => file.relativePath)));
  }, [taskFiles]);
  useEffect(() => {
    if (!snapshot) return;
    setRatioLimitDraft(snapshot.seedRatioLimit != null ? String(snapshot.seedRatioLimit) : "");
    setTimeLimitDraft(snapshot.seedTimeLimitSeconds ?? "");
  }, [snapshot]);
  if (task.protocol !== "bt" && task.protocol !== "magnet") return null;

  const canEditFiles = taskFiles.length > 1 && task.status !== "downloading" && task.status !== "retrying";
  const requiresFileSelection = torrentFileSelectionRequired(task);
  const completedPieces = snapshot ? parseSnapshotNumber(snapshot.completedPieces) : 0;
  const pieceCount = snapshot ? Math.max(0, parseSnapshotNumber(snapshot.pieceCount)) : 0;
  const pieceCells =
    pieceCount > 0
      ? Array.from(
          { length: Math.min(80, pieceCount) },
          (_, index) => index < Math.round((completedPieces / pieceCount) * Math.min(80, pieceCount)),
        )
      : [];

  async function saveFileSelection() {
    setSaving(true);
    try {
      await updateTorrentFileSelection({
        taskId: task.id,
        selectedFilePaths: [...selectedFiles],
      });
    } finally {
      setSaving(false);
    }
  }

  async function toggleSeeding(enabled: boolean) {
    setSaving(true);
    try {
      // FUN-11: do not clear ratio/time policy when flipping the switch.
      await updateTorrentSeeding({
        taskId: task.id,
        enabled,
        ratioLimit: null,
        timeLimitSeconds: null,
        updateLimits: false,
      });
    } finally {
      setSaving(false);
    }
  }

  async function saveSeedingLimits() {
    setSaving(true);
    try {
      const ratio = ratioLimitDraft.trim() === "" ? null : Number(ratioLimitDraft);
      await updateTorrentSeeding({
        taskId: task.id,
        enabled: snapshot?.seedingEnabled ?? false,
        ratioLimit: ratio != null && Number.isFinite(ratio) && ratio > 0 ? ratio : null,
        timeLimitSeconds: timeLimitDraft.trim() === "" ? null : timeLimitDraft.trim(),
        updateLimits: true,
      });
    } finally {
      setSaving(false);
    }
  }

  const fileSelectionPanel =
    taskFiles.length > 1 ? (
      <div
        className={cn(
          "rounded-md bg-surface-root/50 px-3 py-2",
          requiresFileSelection && "border border-border-warning-subtle bg-status-warning/[0.06]",
        )}
        data-bt-file-selection
      >
        <div className="mb-2 flex items-start justify-between gap-2">
          <div className="min-w-0">
            <div className="text-xs font-medium text-text-muted">{t("taskDetails.btFiles")}</div>
            {requiresFileSelection ? (
              <p className="mt-1 text-[11px] leading-4 text-status-warning">{t("errors.btFileSelectionRequired")}</p>
            ) : null}
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!canEditFiles || saving || selectedFiles.size === 0}
            onClick={() => void saveFileSelection()}
          >
            {t("taskDetails.btSaveFiles")}
          </Button>
        </div>
        <div className="max-h-40 space-y-1 overflow-auto pr-1">
          {taskFiles.map((file) => (
            <div key={file.id} className="flex items-center gap-2 text-xs text-text-secondary">
              <Checkbox
                id={`task-file-${file.id}`}
                aria-labelledby={`task-file-label-${file.id}`}
                checked={selectedFiles.has(file.relativePath)}
                disabled={!canEditFiles || saving}
                onChange={(event) => {
                  const next = new Set(selectedFiles);
                  if (event.target.checked) next.add(file.relativePath);
                  else next.delete(file.relativePath);
                  setSelectedFiles(next);
                }}
              />
              <label
                id={`task-file-label-${file.id}`}
                htmlFor={`task-file-${file.id}`}
                className="min-w-0 flex-1 cursor-pointer truncate"
              >
                {file.relativePath}
              </label>
              <span className="ml-auto shrink-0 font-mono text-text-muted">{formatBytes(file.totalSize)}</span>
            </div>
          ))}
        </div>
      </div>
    ) : null;

  if (error) {
    return (
      <div className="space-y-2">
        {fileSelectionPanel}
        <p
          role="alert"
          className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
        >
          {error}
        </p>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <div className="space-y-2">
        <div className="flex items-center justify-between gap-3 px-1">
          <span className="text-xs font-medium text-text-secondary">{t("taskDetails.btRuntime")}</span>
          <span className="text-[11px] text-text-muted">{t("taskDetails.btSpeedLimitUnsupported")}</span>
        </div>
        <p className="rounded-md border border-border-divider bg-surface-root/50 px-3 py-2 text-xs text-text-secondary">
          {t("taskDetails.btNoRuntime")}
        </p>
        {fileSelectionPanel}
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3 px-1">
        <span className="text-xs font-medium text-text-secondary">{t("taskDetails.btRuntime")}</span>
        <span className="text-[11px] text-text-muted">
          {snapshot.seedingEnabled ? t("taskDetails.btSeedingEnabled") : t("taskDetails.btSeedingDisabled")}
        </span>
      </div>
      <div className="space-y-0.5">
        <Row
          label={t("taskDetails.btMetadataStatus")}
          value={snapshot.metadataStatus}
          mono={false}
          hint={t("taskDetails.btMetadataStatusHint")}
        />
        <Row
          label={t("taskDetails.btPeers")}
          value={
            snapshot.seedCount == null
              ? t("taskDetails.btPeersOnly", { peers: snapshot.peerCount })
              : t("taskDetails.btPeersAndSeeds", {
                  peers: snapshot.peerCount,
                  seeds: snapshot.seedCount,
                })
          }
          hint={t("taskDetails.btPeersHint")}
        />
        <Row
          label={t("taskDetails.btPieces")}
          value={`${snapshot.completedPieces} / ${snapshot.pieceCount}`}
          hint={t("taskDetails.btPiecesHint")}
        />
        <div
          role="img"
          aria-label={t("taskDetails.btPiecesAria", {
            completed: completedPieces,
            total: pieceCount,
            percent: pieceCount > 0 ? Math.round((completedPieces / pieceCount) * 100) : 0,
          })}
          className="grid gap-0.5 rounded-md bg-surface-root/50 px-3 py-2"
          style={{ gridTemplateColumns: "repeat(20, minmax(0, 1fr))" }}
        >
          {pieceCells.length > 0 ? (
            pieceCells.map((done, index) => (
              <span key={index} className={cn("h-1.5 rounded-sm", done ? "bg-status-success" : "bg-border-subtle")} />
            ))
          ) : (
            <span className="text-xs text-text-muted">{t("taskDetails.btNoPieces")}</span>
          )}
        </div>
        <Row
          label={t("taskDetails.btUpload")}
          value={`${formatBytes(parseSnapshotNumber(snapshot.uploadBytes))} / ${formatSpeed(parseSnapshotNumber(snapshot.uploadSpeedBps))}`}
          hint={t("taskDetails.btUploadHint")}
        />
        <Row
          label={t("taskDetails.btRatio")}
          value={snapshot.ratio == null ? "-" : snapshot.ratio.toFixed(3)}
          hint={t("taskDetails.btRatioHint")}
        />
        <Row
          label={t("taskDetails.btDht")}
          value={snapshot.dhtStatus ? t("taskDetails.btDhtActive") : t("taskDetails.btDhtUnknown")}
          mono={false}
          hint={t("taskDetails.btDhtHint")}
        />
        {(snapshot.trackers ?? []).length > 0 ? (
          <div className="rounded-md bg-surface-root/50 px-3 py-2 text-xs">
            <div className="mb-1 font-medium text-text-muted">{t("taskDetails.btTrackers")}</div>
            <p className="mb-2 text-[11px] text-text-muted">{t("taskDetails.btTrackersConfiguredOnly")}</p>
            <div className="space-y-1">
              {(snapshot.trackers ?? []).slice(0, 5).map((tracker) => (
                <div key={tracker.url} className="flex items-center justify-between gap-2">
                  <span className="truncate text-text-secondary" title={tracker.url}>
                    {tracker.url}
                  </span>
                  <span className="shrink-0 text-text-muted">{tracker.source ?? tracker.status}</span>
                </div>
              ))}
            </div>
          </div>
        ) : null}
        {snapshot.lastErrorMessage ? (
          <p
            role="alert"
            className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
          >
            {snapshot.lastErrorMessage}
          </p>
        ) : null}
        <div className="flex items-center justify-between gap-3 rounded-md bg-surface-root/50 px-3 py-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                id="bt-seeding-label"
                className="cursor-help rounded-sm text-xs text-text-muted underline decoration-dotted decoration-border-subtle underline-offset-2"
              >
                {t("taskDetails.btSeeding")}
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-64 text-balance">{t("taskDetails.btSeedingHint")}</TooltipContent>
          </Tooltip>
          <Switch
            checked={snapshot.seedingEnabled}
            disabled={saving}
            onCheckedChange={(checked) => void toggleSeeding(checked)}
            aria-labelledby="bt-seeding-label"
          />
        </div>
        <div className="space-y-2 rounded-md bg-surface-root/50 px-3 py-2">
          <div className="text-xs font-medium text-text-muted">{t("taskDetails.btSeedingLimits")}</div>
          <p className="text-[11px] text-text-muted">{t("taskDetails.btSeedingLimitsHint")}</p>
          <div className="grid grid-cols-2 gap-2">
            <label className="space-y-1 text-xs text-text-muted" htmlFor="bt-seed-ratio-limit">
              <span>{t("taskDetails.btSeedRatioLimit")}</span>
              <Input
                id="bt-seed-ratio-limit"
                inputMode="decimal"
                placeholder={t("taskDetails.btSeedLimitUnlimited")}
                value={ratioLimitDraft}
                disabled={saving}
                onChange={(event) => setRatioLimitDraft(event.target.value)}
              />
            </label>
            <label className="space-y-1 text-xs text-text-muted" htmlFor="bt-seed-time-limit">
              <span>{t("taskDetails.btSeedTimeLimit")}</span>
              <Input
                id="bt-seed-time-limit"
                inputMode="numeric"
                placeholder={t("taskDetails.btSeedLimitUnlimited")}
                value={timeLimitDraft}
                disabled={saving}
                onChange={(event) => setTimeLimitDraft(event.target.value)}
              />
            </label>
          </div>
          <Button type="button" size="sm" variant="outline" disabled={saving} onClick={() => void saveSeedingLimits()}>
            {t("taskDetails.btSaveSeedingLimits")}
          </Button>
        </div>
        {fileSelectionPanel}
      </div>
    </div>
  );
}

const TASK_CATEGORY_OPTIONS = ["none", "archive", "image", "video", "document", "installer", "other"] as const;

function AdvancedSettingsDisclosure({ task }: { task: Task }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-1 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent-primary"
        aria-expanded={open}
      >
        <ChevronDown className={cn("h-3.5 w-3.5 shrink-0 transition-transform duration-200", !open && "-rotate-90")} />
        <span>{t("taskDetails.advancedSettings")}</span>
      </button>
      {open ? (
        <div className="space-y-3 pt-1">
          <TaskTransferPanel task={task} />
          <TaskProxyPanel task={task} />
          {/^https?:\/\//i.test(task.url) && !isTorrentProtocol(task.protocol) ? (
            <TaskRequestProfilePanel task={task} />
          ) : null}
          <TaskNetworkAuthorizationPanel task={task} />
        </div>
      ) : null}
    </div>
  );
}

function TaskNetworkAuthorizationPanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [view, setView] = useState<TaskNetworkPolicyView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const editable = task.status !== "downloading" && task.status !== "retrying";

  useEffect(() => {
    let cancelled = false;
    // Older frontend test doubles and embedded adapters may not expose the
    // optional B3 command yet; keep the rest of the details panel usable.
    let load: Promise<TaskNetworkPolicyView>;
    try {
      if (typeof getTaskNetworkPolicy !== "function") return;
      load = getTaskNetworkPolicy(task.id);
    } catch {
      return;
    }
    void load
      .then((value) => {
        if (!cancelled) setView(value);
      })
      .catch((err) => {
        if (!cancelled) setError(errorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, [task.id]);

  const grants = view?.policy.grants ?? [];
  if (grants.length === 0) return null;

  async function revoke() {
    setSaving(true);
    setError(null);
    try {
      setView(await revokeTaskNetworkAuthorization(task.id));
      addToast({ tone: "success", title: t("taskDetails.networkAuthorizationRevoked") });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-2 rounded-md border border-border-subtle bg-surface-raised/40 p-3">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-medium text-text-secondary">{t("taskDetails.networkAuthorization")}</span>
        <Button type="button" size="sm" variant="outline" disabled={!editable || saving} onClick={() => void revoke()}>
          {t("taskDetails.revokeNetworkAuthorization")}
        </Button>
      </div>
      <p className="text-[11px] text-text-muted">
        {grants.map((grant) => `${grant.authority} (${grant.addresses.join(", ")})`).join(" · ")}
      </p>
      {error ? <p className="text-xs text-status-danger">{error}</p> : null}
    </div>
  );
}

function TaskTransferPanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  const upsertTask = useTaskDataStore((s) => s.upsertTask);
  const addToast = useToastStore((s) => s.addToast);
  const initialSpeed = speedLimitInputFromBytes(task.taskSpeedLimitBps);
  const [speedAmount, setSpeedAmount] = useState(initialSpeed.amount);
  const [speedUnit, setSpeedUnit] = useState(initialSpeed.unit);
  const [priority, setPriority] = useState<TaskPriority>(task.priority);
  const [category, setCategory] = useState(task.categoryKey ?? "none");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const nextSpeed = speedLimitInputFromBytes(task.taskSpeedLimitBps);
    setSpeedAmount(nextSpeed.amount);
    setSpeedUnit(nextSpeed.unit);
    setPriority(task.priority);
    setCategory(task.categoryKey ?? "none");
    setError(null);
  }, [task.taskSpeedLimitBps, task.priority, task.categoryKey]);

  const normalizedAmount = speedAmount.trim();
  const currentSpeed = speedLimitInputFromBytes(task.taskSpeedLimitBps);
  const dirty =
    normalizedAmount !== currentSpeed.amount ||
    speedUnit !== currentSpeed.unit ||
    priority !== task.priority ||
    category !== (task.categoryKey ?? "none");

  async function saveTransferOptions() {
    const taskSpeedLimitBps = speedLimitBytesFromInput(speedAmount, speedUnit);
    if (taskSpeedLimitBps === undefined) {
      setError(t("taskDetails.invalidSpeedLimit"));
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const updated = await updateTaskTransferOptions({
        id: task.id,
        taskSpeedLimitBps: taskSpeedLimitBps == null ? null : String(taskSpeedLimitBps),
        priority,
        queuePosition: null,
        categoryKey: category === "none" ? null : category,
        obeySchedule: null,
      });
      upsertTask(updated);
      addToast({ tone: "success", title: t("taskDetails.transferSaved") });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-3 rounded-md border border-border-subtle bg-surface-raised/40 p-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-xs font-medium text-text-secondary">{t("taskDetails.transferSettings")}</div>
          <div className="mt-0.5 text-[11px] text-text-muted">
            {task.status === "downloading" || task.status === "retrying"
              ? t("taskDetails.transferSettingsRunningHint")
              : t("taskDetails.transferSettingsHint")}
          </div>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={saving || !dirty}
          onClick={() => void saveTransferOptions()}
        >
          {saving ? t("taskDetails.savingTransfer") : t("taskDetails.saveTransfer")}
        </Button>
      </div>
      <div className="grid gap-2">
        <label htmlFor="task-speed-limit" className="grid gap-1 text-xs text-text-muted">
          <span>{t("taskDetails.taskSpeedLimit")}</span>
          <div className="grid grid-cols-[minmax(0,1fr)_6rem] gap-2">
            <Input
              id="task-speed-limit"
              value={speedAmount}
              onChange={(event) => setSpeedAmount(event.target.value)}
              inputMode="decimal"
              placeholder={t("taskDetails.unlimited")}
              disabled={saving}
              className="h-8 bg-surface-root text-xs"
            />
            <Select value={speedUnit} onValueChange={setSpeedUnit} disabled={saving}>
              <SelectTrigger aria-label={t("taskDetails.taskSpeedLimit")} className="h-8 bg-surface-root text-xs">
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
        </label>
        <div className="grid grid-cols-2 gap-2">
          <div className="grid gap-1 text-xs text-text-muted">
            <span id="task-priority-label">{t("taskDetails.priority")}</span>
            <Select value={priority} onValueChange={(value) => setPriority(value as TaskPriority)} disabled={saving}>
              <SelectTrigger aria-labelledby="task-priority-label" className="h-8 bg-surface-root text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="high">{t("taskDetails.priorityHigh")}</SelectItem>
                <SelectItem value="normal">{t("taskDetails.priorityNormal")}</SelectItem>
                <SelectItem value="low">{t("taskDetails.priorityLow")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-1 text-xs text-text-muted">
            <span id="task-category-label">{t("taskDetails.categoryLabel")}</span>
            <Select value={category} onValueChange={setCategory} disabled={saving}>
              <SelectTrigger aria-labelledby="task-category-label" className="h-8 bg-surface-root text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TASK_CATEGORY_OPTIONS.map((value) => (
                  <SelectItem key={value} value={value}>
                    {t(`taskDetails.category.${value}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
      </div>
      <div className="flex items-center justify-between gap-3 text-[11px] text-text-muted">
        <span>
          {task.taskSpeedLimitBps
            ? t("taskDetails.currentTaskSpeedLimit", {
                speed: formatSpeed(Number(task.taskSpeedLimitBps)),
              })
            : t("taskDetails.noTaskSpeedLimit")}
        </span>
        <span>{t("taskDetails.queuePosition", { position: task.queuePosition })}</span>
      </div>
      {error ? (
        <p role="alert" className="text-xs text-status-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function TaskProxyPanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<TaskProxySettings | null>(null);
  const [mode, setMode] = useState<TaskProxyMode>("inherit");
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyUsername, setProxyUsername] = useState("");
  const [proxyPassword, setProxyPassword] = useState("");
  const [noProxy, setNoProxy] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const editable = task.status !== "downloading" && task.status !== "retrying";

  useEffect(() => {
    let cancelled = false;
    void getTaskProxySettings(task.id)
      .then((value) => {
        if (cancelled) return;
        setSettings(value);
        setMode(value.mode);
        setProxyUrl(value.proxyUrl);
        setProxyUsername(value.proxyUsername);
        setProxyPassword("");
        setNoProxy(value.noProxy);
        setError(null);
      })
      .catch((err) => {
        if (!cancelled) setError(errorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, [task.id]);

  async function saveProxy() {
    setSaving(true);
    setError(null);
    try {
      const next = await updateTaskProxySettings({
        taskId: task.id,
        mode,
        proxyUrl,
        proxyUsername,
        proxyPassword: proxyPassword.trim() || null,
        clearProxyPassword: false,
        noProxy,
      });
      setSettings(next);
      setProxyPassword("");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="space-y-2 rounded-md border border-border-subtle bg-surface-raised/40 p-3">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-medium text-text-secondary">{t("taskDetails.taskProxy")}</span>
        <span className="text-[11px] text-text-muted">
          {settings?.proxyPasswordSaved ? t("taskDetails.proxyPasswordSaved") : t("taskDetails.proxyPasswordNotSaved")}
        </span>
      </div>
      <Select value={mode} onValueChange={(value) => setMode(value as TaskProxyMode)} disabled={!editable || saving}>
        <SelectTrigger className="h-8" aria-label={t("taskDetails.taskProxy")}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="inherit">{t("taskDetails.proxyInherit")}</SelectItem>
          <SelectItem value="off">{t("taskDetails.proxyOff")}</SelectItem>
          <SelectItem value="custom">{t("taskDetails.proxyCustom")}</SelectItem>
        </SelectContent>
      </Select>
      {mode === "custom" ? (
        <div className="grid gap-2">
          <Input
            value={proxyUrl}
            onChange={(event) => setProxyUrl(event.target.value)}
            placeholder={
              task.protocol === "bt" || task.protocol.startsWith("ftp")
                ? "socks5://127.0.0.1:1080"
                : "http://127.0.0.1:8080"
            }
            aria-label={t("settings.proxyUrl")}
            disabled={!editable || saving}
            className="h-8 bg-surface-root text-xs"
          />
          <div className="grid grid-cols-2 gap-2">
            <Input
              value={proxyUsername}
              onChange={(event) => setProxyUsername(event.target.value)}
              placeholder={t("taskDetails.proxyUsername")}
              aria-label={t("taskDetails.proxyUsername")}
              disabled={!editable || saving}
              className="h-8 bg-surface-root text-xs"
            />
            <Input
              value={proxyPassword}
              onChange={(event) => setProxyPassword(event.target.value)}
              placeholder={t("taskDetails.proxyPassword")}
              aria-label={t("taskDetails.proxyPassword")}
              type="password"
              disabled={!editable || saving}
              className="h-8 bg-surface-root text-xs"
            />
          </div>
          <Input
            value={noProxy}
            onChange={(event) => setNoProxy(event.target.value)}
            placeholder={t("taskDetails.proxyNoProxy")}
            aria-label={t("taskDetails.proxyNoProxy")}
            disabled={!editable || saving}
            className="h-8 bg-surface-root text-xs"
          />
        </div>
      ) : null}
      {error ? (
        <p role="alert" className="text-xs text-status-danger">
          {error}
        </p>
      ) : null}
      <div className="flex items-center justify-between gap-2">
        <p className="text-[11px] text-text-muted">{t("taskDetails.taskProxyHint")}</p>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!editable || saving}
          onClick={() => void saveProxy()}
        >
          {t("taskDetails.saveProxy")}
        </Button>
      </div>
    </div>
  );
}

function DiagnosticsSection({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`${id}-heading`}>
      <h3 id={`${id}-heading`} className="mb-2 text-xs font-semibold text-text-secondary">
        {title}
      </h3>
      {children}
    </section>
  );
}

/**
 * One list for the engine's work units: each row is a connection's byte range
 * with its progress, live speed, and retries. The same data used to sit behind
 * a "By range / By connection" toggle that split it into two half-lists.
 */
const SegmentList = memo(function SegmentList({
  segments,
  taskSpeedBps,
  error,
  emptyLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  segments: TaskSegment[];
  taskSpeedBps: number;
  error: string | null;
  emptyLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p
        role="alert"
        className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
      >
        {error}
      </p>
    );
  }

  if (segments.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  const counts = {
    total: segments.length,
    completed: segments.filter((segment) => segment.status === "completed").length,
    active: segments.filter((segment) => segment.status === "downloading").length,
    failed: segments.filter((segment) => segment.status === "failed").length,
  };

  return (
    <div className="space-y-2 text-xs">
      <p className="text-text-secondary">
        {t("taskDetails.chunksSummary", counts)}
        {counts.active > 0 ? ` · ${formatSpeed(taskSpeedBps, { fixed: true })}` : null}
      </p>
      <ol className="divide-y divide-border-divider rounded-md border border-border-subtle">
        {segments.map((segment, index) => {
          const total = Math.max(1, segment.rangeEnd - segment.rangeStart + 1);
          const completed = Math.max(0, Math.min(total, segment.downloadedUntil - segment.rangeStart));
          const progress = segment.status === "completed" ? 1 : Math.min(1, completed / total);
          const isLive = segment.status === "downloading" || segment.status === "pending";
          const rangeText = `${formatBytes(segment.rangeStart)} – ${formatBytes(segment.rangeEnd + 1)}`;
          const percentText = `${Math.round(progress * 100)}%`;
          const speed = segment.status === "downloading" ? segment.speedBps : 0;

          return (
            <li key={segment.id} className="space-y-1.5 px-3 py-2">
              <div className="flex items-center justify-between gap-3">
                <span className="min-w-0 truncate">
                  <span className="font-medium text-text-primary">
                    {t("taskDetails.connection")} {index + 1}
                  </span>
                  <span className="ml-2 font-mono text-text-muted">{rangeText}</span>
                </span>
                <span className={cn("shrink-0", segmentTone(segment.status))}>
                  {t(`segment.status.${segment.status}`)}
                </span>
              </div>
              <ProgressBar
                value={progress}
                label={t("taskDetails.chunkProgressAria", { range: rangeText, percent: percentText })}
                active={segment.status !== "completed" && segment.status !== "failed"}
                smooth={!isLive}
                tone={segment.status === "failed" ? "danger" : segment.status === "completed" ? "success" : "primary"}
              />
              <div className="flex justify-between gap-3 font-mono text-text-muted">
                <span>
                  {formatBytes(completed, { fixed: true })} / {formatBytes(total)}
                </span>
                <span>{speed > 0 ? formatSpeed(speed, { fixed: true }) : null}</span>
                <span>
                  {t("taskDetails.chunkRetries")} {segment.retryCount}
                </span>
              </div>
              {segment.lastError ? <p className="text-status-danger">{errorMessage(segment.lastError)}</p> : null}
            </li>
          );
        })}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
});

const EventList = memo(function EventList({
  events,
  error,
  emptyLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  events: TaskEvent[];
  error: string | null;
  emptyLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p
        role="alert"
        className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
      >
        {error}
      </p>
    );
  }

  if (events.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  return (
    <div className="space-y-2 text-xs">
      <ol className="space-y-2">
        {events.map((event) => (
          <li key={event.id} className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2">
            <div className="flex items-start justify-between gap-3">
              <span className="font-medium text-text-primary">
                {t(`taskEvent.${event.eventType}`, {
                  defaultValue: event.eventType,
                })}
              </span>
              <time className="shrink-0 font-mono text-[11px] text-text-muted">{formatEventTime(event.createdAt)}</time>
            </div>
            {event.payload ? <EventPayload payload={event.payload} /> : null}
          </li>
        ))}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
});

function EventPayload({ payload }: { payload: string }) {
  const { t } = useTranslation();
  const summary = timelinePayloadSummary(payload, t);
  const localized = summary ?? (parseAppError(payload) ? localizedErrorMessage(payload, t) : t("errors.unknownError"));
  return (
    <div className="mt-1 min-w-0">
      <p className="break-words text-text-secondary">{localized}</p>
      <details className="mt-1 text-[11px] text-text-muted">
        <summary className="cursor-default select-none hover:text-text-secondary">
          {t("recovery.technicalDetails")}
        </summary>
        <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-all rounded bg-surface-root/60 p-2 font-mono text-text-muted">
          {payload}
        </pre>
      </details>
    </div>
  );
}

const RequestList = memo(function RequestList({
  requests,
  error,
  emptyLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  requests: RequestDiagnostic[];
  error: string | null;
  emptyLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p
        role="alert"
        className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
      >
        {error}
      </p>
    );
  }

  if (requests.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  return (
    <div className="space-y-2 text-xs">
      <ol className="space-y-2">
        {requests.map((request) => {
          const httpFields = showsHttpRequestFields(request.method);
          return (
            <li key={request.id} className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2">
              <div className="flex items-start justify-between gap-3">
                <span className="font-mono text-text-primary">
                  <span className="mr-2 inline-flex rounded bg-surface-hover px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-text-secondary">
                    {request.method}
                  </span>
                  {request.statusCode ?? t("taskDetails.requestFailed")}
                </span>
                <time className="shrink-0 font-mono text-[11px] text-text-muted">
                  {formatEventTime(request.createdAt)}
                </time>
              </div>
              <p className="mt-1 break-all font-mono text-[11px] text-text-secondary">{request.url}</p>
              <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-text-muted">
                <span>{t("taskDetails.requestRange")}</span>
                <span className="truncate text-right font-mono text-text-secondary" title={request.rangeHeader ?? "-"}>
                  {request.rangeHeader ?? "-"}
                </span>
                {httpFields ? (
                  <>
                    <span>{t("taskDetails.requestIfRange")}</span>
                    <span
                      className="truncate text-right font-mono text-text-secondary"
                      title={request.ifRangeHeader ?? "-"}
                    >
                      {request.ifRangeHeader ?? "-"}
                    </span>
                  </>
                ) : null}
                <span>{t("taskDetails.requestLength")}</span>
                <span className="text-right font-mono text-text-secondary">
                  {request.contentLength ? formatBytes(Number(request.contentLength)) : "-"}
                </span>
                <span>{t("taskDetails.requestDuration")}</span>
                <span className="text-right font-mono text-text-secondary">{request.durationMs} ms</span>
                <span>{t("taskDetails.requestRetries")}</span>
                <span className="text-right font-mono text-text-secondary">{request.retryCount}</span>
              </div>
              {httpFields && request.etag ? (
                <p className="mt-2 break-all font-mono text-[11px] text-text-muted">ETag {request.etag}</p>
              ) : null}
              {request.errorMessage ? (
                <p role="alert" className="mt-2 text-status-danger">
                  {request.errorMessage}
                </p>
              ) : null}
            </li>
          );
        })}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
});

const HlsSegmentList = memo(function HlsSegmentList({
  segments,
  error,
  emptyLabel,
  sequenceLabel,
  statusLabel,
  durationLabel,
  retriesLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  segments: HlsSegmentView[];
  error: string | null;
  emptyLabel: string;
  sequenceLabel: string;
  statusLabel: string;
  durationLabel: string;
  retriesLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p
        role="alert"
        className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
      >
        {error}
      </p>
    );
  }

  if (segments.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  return (
    <div className="space-y-2 text-xs">
      <ol className="space-y-2">
        {segments.map((segment) => (
          <li key={segment.id} className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2">
            <div className="flex items-start justify-between gap-3">
              <span className="font-mono text-text-primary">
                #{segment.mediaSequence}
                {Number(segment.discontinuitySequence) > 0 ? ` (d${segment.discontinuitySequence})` : ""}
              </span>
              <span className="shrink-0 text-text-secondary">{t(`segment.status.${segment.status}`)}</span>
            </div>
            <p className="mt-1 break-all font-mono text-[11px] text-text-secondary">{segment.uri}</p>
            <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-text-muted">
              <span>{sequenceLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.mediaSequence}</span>
              <span>{statusLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.status}</span>
              <span>{durationLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.durationMs} ms</span>
              <span>{retriesLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.retryCount}</span>
            </div>
            {segment.lastError ? (
              <p role="alert" className="mt-2 text-status-danger">
                {segment.lastError}
              </p>
            ) : null}
          </li>
        ))}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
});

const DashSegmentList = memo(function DashSegmentList({
  segments,
  error,
  emptyLabel,
  trackLabel,
  indexLabel,
  statusLabel,
  retriesLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  segments: DashSegmentView[];
  error: string | null;
  emptyLabel: string;
  trackLabel: string;
  indexLabel: string;
  statusLabel: string;
  retriesLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p
        role="alert"
        className="rounded-md border border-border-danger bg-status-danger/10 px-3 py-2 text-xs text-status-danger"
      >
        {error}
      </p>
    );
  }

  if (segments.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  return (
    <div className="space-y-2 text-xs">
      <ol className="space-y-2">
        {segments.map((segment) => (
          <li key={segment.id} className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2">
            <div className="flex items-start justify-between gap-3">
              <span className="font-mono text-text-primary">
                {segment.trackKind} #{segment.segmentIndex}
              </span>
              <span className="shrink-0 text-text-secondary">{t(`segment.status.${segment.status}`)}</span>
            </div>
            <p className="mt-1 break-all font-mono text-[11px] text-text-secondary">{segment.uri}</p>
            <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-text-muted">
              <span>{trackLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.trackKind}</span>
              <span>{indexLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.segmentIndex}</span>
              <span>{statusLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.status}</span>
              <span>{retriesLabel}</span>
              <span className="text-right font-mono text-text-secondary">{segment.retryCount}</span>
            </div>
            {segment.lastError ? (
              <p role="alert" className="mt-2 text-status-danger">
                {segment.lastError}
              </p>
            ) : null}
          </li>
        ))}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
});

function LoadMoreButton({ visible, label, onClick }: { visible: boolean; label: string; onClick: () => void }) {
  if (!visible) return null;
  return (
    <Button type="button" variant="outline" size="sm" className="w-full" onClick={onClick}>
      {label}
    </Button>
  );
}

function formatEventTime(value: string): string {
  return formatDateTime(value, "dateTimeSeconds");
}

function parseSnapshotNumber(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

function hashTone(status: Task["hashStatus"]): string {
  switch (status) {
    case "verified":
      return "text-status-success";
    case "failed":
      return "text-status-danger";
    case "pending":
      return "text-status-warning";
    default:
      return "text-text-secondary";
  }
}

function segmentTone(status: TaskSegment["status"]): string {
  switch (status) {
    case "completed":
      return "text-status-success";
    case "failed":
      return "text-status-danger";
    case "downloading":
      return "text-accent-primary";
    default:
      return "text-text-muted";
  }
}
