import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Button } from "@/components/ui/button";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";
import { listTaskSegments } from "@/lib/tauri";
import { formatBytes, formatEta, formatPercent, formatSpeed } from "@/lib/utils";
import { cn } from "@/lib/utils";
import { ProgressBar } from "@/components/ui/progress-bar";
import { useIsCompactShell } from "@/hooks/use-shell-layout";

const SEGMENT_REFRESH_MS = 2500;

interface TaskDetailsProps {
  task: Task | null;
  open: boolean;
  onClose?: () => void;
}

export function TaskDetails({ task, open, onClose }: TaskDetailsProps) {
  const compact = useIsCompactShell();

  if (!open || !task) return null;

  if (compact) {
    return (
      <TaskDetailsDrawer task={task} open={open} onClose={onClose} />
    );
  }

  return (
    <aside
      className={cn(
        "flex w-80 shrink-0 flex-col border-l border-border-subtle bg-surface-base xl:w-96",
        "motion-safe:animate-[detail-enter_220ms_cubic-bezier(0.16,1,0.3,1)_both]",
      )}
    >
      <TaskDetailsHeader task={task} />
      <TaskDetailsPanel task={task} />
    </aside>
  );
}

function TaskDetailsDrawer({
  task,
  open,
  onClose,
}: {
  task: Task;
  open: boolean;
  onClose?: () => void;
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
          <TaskDetailsPanel task={task} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function TaskDetailsHeader({ task }: { task: Task }) {
  return (
    <header className="flex shrink-0 items-start gap-2 border-b border-border-subtle px-4 py-3">
      <div className="min-w-0 flex-1">
        <h2 className="truncate text-sm font-medium">{task.fileName}</h2>
        <p className="truncate text-xs text-text-muted">{task.saveDir}</p>
      </div>
    </header>
  );
}

function TaskDetailsPanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState("overview");
  const [segments, setSegments] = useState<TaskSegment[]>([]);
  const [segmentError, setSegmentError] = useState<string | null>(null);

  useEffect(() => {
    setActiveTab("overview");
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
      void listTaskSegments(task.id)
        .then((nextSegments) => {
          if (!cancelled) {
            setSegments(nextSegments);
            setSegmentError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setSegmentError(String(error));
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
        </TabsList>
        <ScrollArea className="min-h-0 flex-1">
          <TabsContent value="overview" className="space-y-2 text-sm">
            <Row label={t("taskDetails.progress")} value={formatPercent(task.downloadedBytes, task.totalSize)} />
            <Row label={t("taskDetails.speed")} value={formatSpeed(task.speedBps)} />
            <Row label={t("taskDetails.eta")} value={formatEta(task.downloadedBytes, task.totalSize, task.speedBps)} />
          </TabsContent>
          <TabsContent value="chunks">
            <ChunkList
              segments={segments}
              error={segmentError}
              emptyLabel={t("taskDetails.noChunks")}
              rangeLabel={t("taskDetails.chunkRange")}
              progressLabel={t("taskDetails.chunkProgress")}
              retryLabel={t("taskDetails.chunkRetries")}
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

function ChunkList({
  segments,
  error,
  emptyLabel,
  rangeLabel,
  progressLabel,
  retryLabel,
}: {
  segments: TaskSegment[];
  error: string | null;
  emptyLabel: string;
  rangeLabel: string;
  progressLabel: string;
  retryLabel: string;
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
      {segments.map((segment) => {
        const total = Math.max(1, segment.rangeEnd - segment.rangeStart + 1);
        const completed = Math.max(0, segment.downloadedUntil - segment.rangeStart);
        const progress = Math.min(1, completed / total);
        const isLive =
          segment.status === "downloading" || segment.status === "pending";

        return (
          <div
            key={segment.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 p-3"
          >
            <div className="flex items-center justify-between gap-3">
              <span className="font-medium text-text-primary">
                {rangeLabel} {formatBytes(segment.rangeStart)} - {formatBytes(segment.rangeEnd)}
              </span>
              <span className={cn("capitalize", segmentTone(segment.status))}>
                {t(`segment.status.${segment.status}`)}
              </span>
            </div>
            <div className="mt-2">
              <ProgressBar
                value={progress}
                label={t("taskDetails.chunkProgressAria", {
                  range: `${formatBytes(segment.rangeStart)} - ${formatBytes(segment.rangeEnd)}`,
                  percent: `${Math.round(progress * 100)}%`,
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
}: {
  segments: TaskSegment[];
  taskSpeedBps: number;
  error: string | null;
  emptyLabel: string;
  connectionLabel: string;
  rangeLabel: string;
  progressLabel: string;
  speedLabel: string;
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
  const averageActiveSpeed =
    activeSegments.length > 0 ? taskSpeedBps / activeSegments.length : 0;

  return (
    <div className="space-y-2 text-xs">
      {segments.map((segment, index) => {
        const total = Math.max(1, segment.rangeEnd - segment.rangeStart + 1);
        const completed = Math.max(
          0,
          segment.downloadedUntil - segment.rangeStart,
        );
        const speed =
          segment.status === "downloading" ? averageActiveSpeed : 0;

        return (
          <div
            key={segment.id}
            className="rounded-md border border-border-subtle bg-surface-raised/50 p-3"
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
                {formatBytes(segment.rangeStart)} - {formatBytes(segment.rangeEnd)}
              </span>
              <span>{progressLabel}</span>
              <span className="text-right font-mono text-text-secondary">
                {formatPercent(completed, total)}
              </span>
              <span>{speedLabel}</span>
              <span className="text-right font-mono text-text-secondary">
                {formatSpeed(speed)}
              </span>
            </div>
            {segment.lastError ? (
              <p className="mt-2 text-status-danger">{segment.lastError}</p>
            ) : null}
          </div>
        );
      })}
    </div>
  );
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
