import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Button } from "@/components/ui/button";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import type { HashVerificationState, RecoveryAction, RequestDiagnostic, TaskEvent } from "@/generated/bindings";
import {
  listSegmentsPage,
  listTaskEventsPage,
  listTaskRequestsPage,
  verifyTaskHash,
} from "@/lib/tauri";
import { errorMessage } from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { ProgressBar } from "@/components/ui/progress-bar";
import { useIsCompactShell } from "@/hooks/use-shell-layout";
import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";

const log = createLogger("task-details");

const SEGMENT_REFRESH_MS = 2500;

interface TaskDetailsProps {
  task: Task | null;
  open: boolean;
  onClose?: () => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
}

export function TaskDetails({ task, open, onClose, onResolveAttention }: TaskDetailsProps) {
  const compact = useIsCompactShell();

  if (!open || !task) return null;

  if (compact) {
    return (
      <TaskDetailsDrawer
        task={task}
        open={open}
        onClose={onClose}
        onResolveAttention={onResolveAttention}
      />
    );
  }

  return (
    <aside
      className={cn(
        "flex w-80 shrink-0 flex-col border-l border-border-subtle bg-surface-base xl:w-96",
        "motion-safe:animate-[detail-enter_220ms_cubic-bezier(0.16,1,0.3,1)_both]",
      )}
      aria-labelledby="task-details-heading"
    >
      <TaskDetailsHeader task={task} />
      <TaskDetailsPanel task={task} onResolveAttention={onResolveAttention} />
    </aside>
  );
}

function TaskDetailsDrawer({
  task,
  open,
  onClose,
  onResolveAttention,
}: {
  task: Task;
  open: boolean;
  onClose?: () => void;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
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
        <Dialog.Overlay className="fixed inset-0 z-40 bg-surface-scrim motion-safe:animate-[fade-in_180ms_ease-out]" />
        <Dialog.Content
          className={cn(
            "fixed inset-y-0 right-0 z-50 flex h-[100dvh] w-full max-w-sm flex-col border-l border-border-subtle bg-surface-base shadow-xl",
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
              <Dialog.Title className="truncate text-sm font-medium">
                {task.fileName}
              </Dialog.Title>
              <p className="truncate text-xs text-text-muted">{task.saveDir}</p>
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
          <TaskDetailsPanel task={task} onResolveAttention={onResolveAttention} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function TaskDetailsHeader({ task }: { task: Task }) {
  return (
    <header className="flex shrink-0 items-start gap-2 border-b border-border-subtle px-4 py-3">
      <div className="min-w-0 flex-1">
        <h2 id="task-details-heading" className="truncate text-sm font-medium">{task.fileName}</h2>
        <p className="truncate text-xs text-text-muted">{task.saveDir}</p>
      </div>
    </header>
  );
}

function TaskDetailsPanel({
  task,
  onResolveAttention,
}: {
  task: Task;
  onResolveAttention: (task: Task, action: RecoveryAction) => void;
}) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState("overview");
  const [segments, setSegments] = useState<TaskSegment[]>([]);
  const [segmentsCursor, setSegmentsCursor] = useState<string | null>(null);
  const [segmentError, setSegmentError] = useState<string | null>(null);
  const [events, setEvents] = useState<TaskEvent[]>([]);
  const [eventsCursor, setEventsCursor] = useState<string | null>(null);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [requests, setRequests] = useState<RequestDiagnostic[]>([]);
  const [requestsCursor, setRequestsCursor] = useState<string | null>(null);
  const [requestsError, setRequestsError] = useState<string | null>(null);
  const [hashState, setHashState] = useState<HashVerificationState | null>(null);
  const [verifyingHash, setVerifyingHash] = useState(false);

  useEffect(() => {
    setActiveTab("overview");
    setHashState(null);
    setSegments([]);
    setSegmentsCursor(null);
    setEvents([]);
    setEventsCursor(null);
    setRequests([]);
    setRequestsCursor(null);
  }, [task.id]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (activeTab !== "chunks" && activeTab !== "connections") {
      setSegments([]);
      setSegmentError(null);
      return;
    }

    const loadSegments = () => {
      void listSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setSegments(result.items);
            setSegmentsCursor(result.nextCursor);
            setSegmentError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setSegmentError(errorMessage(error));
        });
    };

    loadSegments();

    const isLive =
      task.status === "downloading" || task.status === "retrying";
    if (isLive) {
      intervalId = setInterval(loadSegments, SEGMENT_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, task.id, task.status]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (activeTab !== "requests") {
      setRequests([]);
      setRequestsError(null);
      return;
    }

    const loadRequests = () => {
      void listTaskRequestsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setRequests(result.items);
            setRequestsCursor(result.nextCursor);
            setRequestsError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setRequestsError(errorMessage(error));
        });
    };

    loadRequests();

    const isLive =
      task.status === "downloading" ||
      task.status === "retrying" ||
      task.status === "queued";
    if (isLive) {
      intervalId = setInterval(loadRequests, SEGMENT_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, task.id, task.status]);

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

  async function loadMoreSegments() {
    if (!segmentsCursor) return;
    try {
      const result = await listSegmentsPage({
        taskId: task.id,
        cursor: segmentsCursor,
        pageSize: 100,
      });
      setSegments((current) => mergeById(current, result.items));
      setSegmentsCursor(result.nextCursor);
      setSegmentError(null);
    } catch (error) {
      setSegmentError(errorMessage(error));
    }
  }

  async function loadMoreEvents() {
    if (!eventsCursor) return;
    try {
      const result = await listTaskEventsPage({
        taskId: task.id,
        cursor: eventsCursor,
        pageSize: 100,
      });
      setEvents((current) => mergeById(current, result.items));
      setEventsCursor(result.nextCursor);
      setEventsError(null);
    } catch (error) {
      setEventsError(errorMessage(error));
    }
  }

  async function loadMoreRequests() {
    if (!requestsCursor) return;
    try {
      const result = await listTaskRequestsPage({
        taskId: task.id,
        cursor: requestsCursor,
        pageSize: 100,
      });
      setRequests((current) => mergeById(current, result.items));
      setRequestsCursor(result.nextCursor);
      setRequestsError(null);
    } catch (error) {
      setRequestsError(errorMessage(error));
    }
  }

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (activeTab !== "logs") {
      setEvents([]);
      setEventsError(null);
      return;
    }

    const loadEvents = () => {
      void listTaskEventsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setEvents(result.items);
            setEventsCursor(result.nextCursor);
            setEventsError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setEventsError(errorMessage(error));
        });
    };

    loadEvents();

    const isLive =
      task.status === "downloading" ||
      task.status === "retrying" ||
      task.status === "queued";
    if (isLive) {
      intervalId = setInterval(loadEvents, SEGMENT_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, task.id, task.status]);

  return (
    <Tabs
      value={activeTab}
      onValueChange={setActiveTab}
      className="flex min-h-0 flex-1 flex-col px-4 py-3"
    >
        <TabsList className="w-full justify-start overflow-x-auto">
          <TabsTrigger value="overview">{t("taskDetails.overview")}</TabsTrigger>
          <TabsTrigger value="chunks">{t("taskDetails.chunks")}</TabsTrigger>
          <TabsTrigger value="connections">{t("taskDetails.connections")}</TabsTrigger>
          <TabsTrigger value="requests">{t("taskDetails.requests")}</TabsTrigger>
          <TabsTrigger value="logs">{t("taskDetails.logs")}</TabsTrigger>
        </TabsList>
        <ScrollArea className="min-h-0 flex-1">
          <TabsContent value="overview" className="space-y-2 text-sm">
            <Row label={t("taskDetails.progress")} value={formatPercent(task.downloadedBytes, task.totalSize)} />
            <Row label={t("taskDetails.speed")} value={formatSpeed(task.speedBps)} />
            <Row label={t("taskDetails.eta")} value={formatEta(task.downloadedBytes, task.totalSize, task.speedBps)} />
            <HashPanel
              task={task}
              state={hashState}
              verifying={verifyingHash}
              onVerify={() => void runHashVerification()}
            />
            <TaskRecoveryActions task={task} onResolve={onResolveAttention} />
          </TabsContent>
          <TabsContent value="chunks">
            <ChunkList
              segments={segments}
              error={segmentError}
              emptyLabel={t("taskDetails.noChunks")}
              rangeLabel={t("taskDetails.chunkRange")}
              progressLabel={t("taskDetails.chunkProgress")}
              retryLabel={t("taskDetails.chunkRetries")}
              hasMore={Boolean(segmentsCursor)}
              loadMoreLabel={t("taskDetails.loadMore")}
              onLoadMore={() => void loadMoreSegments()}
            />
          </TabsContent>
          <TabsContent value="connections">
            <ConnectionList
              segments={segments}
              taskSpeedBps={task.speedBps}
              error={segmentError}
              emptyLabel={t("taskDetails.noConnections")}
              connectionLabel={t("taskDetails.connection")}
              rangeLabel={t("taskDetails.connectionRange")}
              progressLabel={t("taskDetails.connectionProgress")}
              speedLabel={t("taskDetails.connectionSpeed")}
              hasMore={Boolean(segmentsCursor)}
              loadMoreLabel={t("taskDetails.loadMore")}
              onLoadMore={() => void loadMoreSegments()}
            />
          </TabsContent>
          <TabsContent value="requests">
            <RequestList
              requests={requests}
              error={requestsError}
              emptyLabel={t("taskDetails.noRequests")}
              hasMore={Boolean(requestsCursor)}
              loadMoreLabel={t("taskDetails.loadMore")}
              onLoadMore={() => void loadMoreRequests()}
            />
          </TabsContent>
          <TabsContent value="logs">
            <EventList
              events={events}
              error={eventsError}
              emptyLabel={t("taskDetails.noLogs")}
              hasMore={Boolean(eventsCursor)}
              loadMoreLabel={t("taskDetails.loadMore")}
              onLoadMore={() => void loadMoreEvents()}
            />
          </TabsContent>
        </ScrollArea>
      </Tabs>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-text-muted">{label}</div>
      <div className={cn("text-text-primary")}>{value}</div>
    </div>
  );
}

function HashPanel({
  task,
  state,
  verifying,
  onVerify,
}: {
  task: Task;
  state: HashVerificationState | null;
  verifying: boolean;
  onVerify: () => void;
}) {
  const { t } = useTranslation();
  const status = state?.status ?? task.hashStatus;
  const actual = state?.actualSha256 ?? task.actualHashSha256;
  const error = state?.errorMessage ?? task.hashError;

  if (!task.expectedHashSha256 && !state?.expectedSha256) {
    return (
      <Row
        label={t("taskDetails.integrity")}
        value={t("taskDetails.hashNotRequested")}
      />
    );
  }

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-text-muted">{t("taskDetails.integrity")}</div>
          <div className={cn("font-medium", hashTone(status))}>
            {t(`hash.status.${status}`)}
          </div>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-8 shrink-0"
          onClick={onVerify}
          disabled={verifying || task.status !== "completed"}
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
      {error ? <p className="mt-2 text-status-danger">{error}</p> : null}
    </div>
  );
}

function ChunkList({
  segments,
  error,
  emptyLabel,
  rangeLabel,
  progressLabel,
  retryLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  segments: TaskSegment[];
  error: string | null;
  emptyLabel: string;
  rangeLabel: string;
  progressLabel: string;
  retryLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
        {error}
      </p>
    );
  }

  if (segments.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  return (
    <div className="space-y-3 text-xs">
      <p className="rounded-md border border-border-subtle/70 bg-surface-root/50 px-3 py-2 text-text-secondary">
        {t("taskDetails.chunksSummary", {
          total: segments.length,
          completed: segments.filter((segment) => segment.status === "completed").length,
          active: segments.filter((segment) => segment.status === "downloading").length,
          failed: segments.filter((segment) => segment.status === "failed").length,
        })}
      </p>
      {segments.map((segment) => {
        const total = Math.max(1, segment.rangeEnd - segment.rangeStart + 1);
        const completed = Math.max(0, segment.downloadedUntil - segment.rangeStart);
        const progress = Math.min(1, completed / total);
        const isLive =
          segment.status === "downloading" || segment.status === "pending";
        const rangeText = `${formatBytes(segment.rangeStart)} - ${formatBytes(segment.rangeEnd)}`;
        const percentText = `${Math.round(progress * 100)}%`;

        return (
          <div
            key={segment.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 p-3"
            title={t("taskDetails.chunkTooltip", {
              range: rangeText,
              percent: percentText,
              retries: segment.retryCount,
            })}
          >
            <div className="flex items-center justify-between gap-3">
              <span className="font-medium text-text-primary">
                {rangeLabel} {rangeText}
              </span>
              <span className={cn("capitalize", segmentTone(segment.status))}>
                {t(`segment.status.${segment.status}`)}
              </span>
            </div>
            <div className="mt-2">
              <ProgressBar
                value={progress}
                label={t("taskDetails.chunkProgressAria", {
                  range: rangeText,
                  percent: percentText,
                })}
                active={segment.status !== "completed" && segment.status !== "failed"}
                smooth={!isLive}
                tone={
                  segment.status === "failed"
                    ? "danger"
                    : segment.status === "completed"
                      ? "success"
                      : "primary"
                }
                className="h-1.5"
              />
            </div>
            <div className="mt-2 flex justify-between gap-3 text-text-muted">
              <span>
                {progressLabel} {formatBytes(completed)} / {formatBytes(total)}
              </span>
              <span>
                {retryLabel} {segment.retryCount}
              </span>
            </div>
            {segment.lastError ? (
              <p className="mt-2 text-status-danger">{segment.lastError}</p>
            ) : null}
          </div>
        );
      })}
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
}

function ConnectionList({
  segments,
  taskSpeedBps,
  error,
  emptyLabel,
  connectionLabel,
  rangeLabel,
  progressLabel,
  speedLabel,
  hasMore,
  loadMoreLabel,
  onLoadMore,
}: {
  segments: TaskSegment[];
  taskSpeedBps: number;
  error: string | null;
  emptyLabel: string;
  connectionLabel: string;
  rangeLabel: string;
  progressLabel: string;
  speedLabel: string;
  hasMore: boolean;
  loadMoreLabel: string;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
        {error}
      </p>
    );
  }

  if (segments.length === 0) {
    return <p className="text-xs text-text-secondary">{emptyLabel}</p>;
  }

  const activeSegments = segments.filter(
    (segment) => segment.status === "downloading",
  );
  return (
    <div className="space-y-2 text-xs">
      <p className="rounded-md border border-border-subtle/70 bg-surface-root/50 px-3 py-2 text-text-secondary">
        {t("taskDetails.connectionsSummary", {
          total: segments.length,
          active: activeSegments.length,
          speed: formatSpeed(taskSpeedBps),
        })}
      </p>
      {segments.map((segment, index) => {
        const total = Math.max(1, segment.rangeEnd - segment.rangeStart + 1);
        const completed = Math.max(
          0,
          segment.downloadedUntil - segment.rangeStart,
        );
        const speed = segment.status === "downloading" ? segment.speedBps : 0;
        const rangeText = `${formatBytes(segment.rangeStart)} - ${formatBytes(segment.rangeEnd)}`;
        const percentText = formatPercent(completed, total);

        return (
          <div
            key={segment.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 p-3"
            title={t("taskDetails.connectionTooltip", {
              index: index + 1,
              range: rangeText,
              percent: percentText,
              speed: formatSpeed(speed),
            })}
          >
            <div className="flex items-center justify-between gap-3">
              <span className="font-medium text-text-primary">
                {connectionLabel} {index + 1}
              </span>
              <span className={cn("capitalize", segmentTone(segment.status))}>
                {t(`segment.status.${segment.status}`)}
              </span>
            </div>
            <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-2 text-text-muted">
              <span>{rangeLabel}</span>
              <span className="text-right font-mono text-text-secondary">
                {rangeText}
              </span>
              <span>{progressLabel}</span>
              <span className="text-right font-mono text-text-secondary">
                {percentText}
              </span>
              <span>{speedLabel}</span>
              <span className="text-right font-mono text-text-secondary">
                {formatSpeed(speed)}
              </span>
            </div>
            {segment.lastError ? (
              <p className="mt-2 text-status-danger">{errorMessage(segment.lastError)}</p>
            ) : null}
          </div>
        );
      })}
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
}

function EventList({
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
      <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
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
          <li
            key={event.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2"
          >
            <div className="flex items-start justify-between gap-3">
              <span className="font-medium text-text-primary">
                {t(`taskEvent.${event.eventType}`, {
                  defaultValue: event.eventType,
                })}
              </span>
              <time className="shrink-0 font-mono text-[11px] text-text-muted">
                {formatEventTime(event.createdAt)}
              </time>
            </div>
            {event.payload ? (
              <p className="mt-1 break-words text-text-secondary">{event.payload}</p>
            ) : null}
          </li>
        ))}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
}

function RequestList({
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
      <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
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
        {requests.map((request) => (
          <li
            key={request.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 px-3 py-2"
          >
          <div className="flex items-start justify-between gap-3">
            <span className="font-mono text-text-primary">
              {request.method} {request.statusCode ?? t("taskDetails.requestFailed")}
            </span>
            <time className="shrink-0 font-mono text-[11px] text-text-muted">
              {formatEventTime(request.createdAt)}
            </time>
          </div>
          <p className="mt-1 break-all font-mono text-[11px] text-text-secondary">
            {request.url}
          </p>
          <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-text-muted">
            <span>{t("taskDetails.requestRange")}</span>
            <span className="truncate text-right font-mono text-text-secondary">
              {request.rangeHeader ?? "-"}
            </span>
            <span>{t("taskDetails.requestLength")}</span>
            <span className="text-right font-mono text-text-secondary">
              {request.contentLength ? formatBytes(Number(request.contentLength)) : "-"}
            </span>
            <span>{t("taskDetails.requestDuration")}</span>
            <span className="text-right font-mono text-text-secondary">
              {request.durationMs} ms
            </span>
            <span>{t("taskDetails.requestRetries")}</span>
            <span className="text-right font-mono text-text-secondary">
              {request.retryCount}
            </span>
          </div>
          {request.etag ? (
            <p className="mt-2 break-all font-mono text-[11px] text-text-muted">
              ETag {request.etag}
            </p>
          ) : null}
          {request.errorMessage ? (
            <p className="mt-2 text-status-danger">{request.errorMessage}</p>
          ) : null}
          </li>
        ))}
      </ol>
      <LoadMoreButton visible={hasMore} label={loadMoreLabel} onClick={onLoadMore} />
    </div>
  );
}

function LoadMoreButton({
  visible,
  label,
  onClick,
}: {
  visible: boolean;
  label: string;
  onClick: () => void;
}) {
  if (!visible) return null;
  return (
    <Button type="button" variant="outline" size="sm" className="w-full" onClick={onClick}>
      {label}
    </Button>
  );
}

function mergeById<T extends { id: string }>(current: T[], incoming: T[]): T[] {
  const byId = new Map(current.map((item) => [item.id, item] as const));
  const order = current.map((item) => item.id);
  for (const item of incoming) {
    if (!byId.has(item.id)) order.push(item.id);
    byId.set(item.id, item);
  }
  return order.map((id) => byId.get(id)).filter((item): item is T => Boolean(item));
}

function formatEventTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
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
