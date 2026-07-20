import {
  ArrowDown,
  ArrowUp,
  CheckCircle2,
  ChevronDown,
  ChevronsDown,
  ChevronsUp,
  CircleGauge,
  Clock3,
  ExternalLink,
  ListOrdered,
  Pause,
  RefreshCcw,
  Server,
} from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";

import type { ReorderAction } from "@/components/tasks/TaskContextMenu";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { QueueTaskDecision, QueueWaitReason, SchedulerSnapshot, TaskPriority } from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { getSchedulerSnapshot } from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

const PRIORITY_ORDER: TaskPriority[] = ["high", "normal", "low"];

export function QueueCenter({
  taskIds,
  loading,
  error,
  hasMore,
  onLoadMore,
  onRetryLoad,
  onPause,
  onReorder,
  onShowDetails,
  onUpdateOptions,
}: {
  taskIds: string[];
  loading: boolean;
  error: string | null;
  hasMore: boolean;
  onLoadMore: () => void;
  onRetryLoad: () => void;
  onPause: (task: Task) => void;
  onReorder?: (task: Task, action: ReorderAction) => void;
  onShowDetails?: (task: Task) => void;
  onUpdateOptions: (task: Task, patch: { priority?: TaskPriority; obeySchedule?: boolean }) => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const selectedId = useTaskUIStore((state) => state.selectedId);
  const selectTask = useTaskUIStore((state) => state.selectTask);
  const setNav = useTaskUIStore((state) => state.setNav);
  const tasks = useTaskDataStore(
    useShallow((state) =>
      taskIds.map((id) => state.taskById[id]).filter((task): task is Task => Boolean(task) && task.status === "queued"),
    ),
  );
  const [snapshot, setSnapshot] = useState<SchedulerSnapshot | null>(null);
  const [snapshotError, setSnapshotError] = useState<string | null>(null);
  const [snapshotLoading, setSnapshotLoading] = useState(true);
  const [updatingTaskId, setUpdatingTaskId] = useState<string | null>(null);
  const queueIdsKey = tasks.map((task) => task.id).join("\u0000");
  const selectedTask = tasks.find((task) => task.id === selectedId) ?? tasks[0] ?? null;

  useEffect(() => {
    if (selectedTask && selectedTask.id !== selectedId) selectTask(selectedTask.id);
  }, [selectTask, selectedId, selectedTask]);

  const refreshSnapshot = useCallback(async () => {
    setSnapshotLoading(true);
    try {
      const next = await getSchedulerSnapshot(queueIdsKey ? queueIdsKey.split("\u0000") : []);
      setSnapshot(next);
      setSnapshotError(null);
    } catch (nextError) {
      setSnapshotError(errorMessage(nextError));
    } finally {
      setSnapshotLoading(false);
    }
  }, [queueIdsKey]);

  useEffect(() => {
    void refreshSnapshot();
    const timer = window.setInterval(() => void refreshSnapshot(), 10_000);
    return () => window.clearInterval(timer);
  }, [refreshSnapshot]);

  const decisions = useMemo(
    () => new Map(snapshot?.decisions.map((decision) => [decision.taskId, decision]) ?? []),
    [snapshot],
  );
  const groups = PRIORITY_ORDER.map((priority) => ({
    priority,
    tasks: tasks.filter((task) => task.priority === priority),
  })).filter((group) => group.tasks.length > 0);

  // UX-10: single flat order for roving tabindex across priority sections.
  const focusTask = (taskId: string) => {
    selectTask(taskId);
    requestAnimationFrame(() => {
      document.getElementById(`queue-task-${taskId}`)?.focus();
    });
  };

  const handleQueueListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (tasks.length === 0) return;
    const currentIndex = Math.max(
      0,
      tasks.findIndex((task) => task.id === selectedTask?.id),
    );
    let nextIndex = currentIndex;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      nextIndex = Math.min(tasks.length - 1, currentIndex + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      nextIndex = Math.max(0, currentIndex - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      nextIndex = 0;
    } else if (event.key === "End") {
      event.preventDefault();
      nextIndex = tasks.length - 1;
    } else {
      return;
    }
    const next = tasks[nextIndex];
    if (next) focusTask(next.id);
  };

  const updateOptions = async (task: Task, patch: { priority?: TaskPriority; obeySchedule?: boolean }) => {
    setUpdatingTaskId(task.id);
    try {
      const success = await onUpdateOptions(task, patch);
      if (success) await refreshSnapshot();
    } finally {
      setUpdatingTaskId(null);
    }
  };

  const schedulerDetails = (idPrefix: string) => (
    <SchedulerDetails
      idPrefix={idPrefix}
      snapshot={snapshot}
      snapshotError={snapshotError}
      snapshotLoading={snapshotLoading}
      selectedTask={selectedTask}
      selectedDecision={selectedTask ? (decisions.get(selectedTask.id) ?? null) : null}
      updating={updatingTaskId === selectedTask?.id}
      onRefresh={refreshSnapshot}
      onUpdateOptions={updateOptions}
      onReorder={onReorder}
      onPause={onPause}
      onShowDetails={onShowDetails}
    />
  );

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root" aria-labelledby="queue-title">
      <header className="flex min-h-12 items-center gap-3 border-b border-border-divider px-3 py-2 md:px-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <h1 id="queue-title" className="text-base font-semibold leading-5 text-text-primary">
              {t("queueCenter.title")}
            </h1>
            <span className="font-mono text-xs text-text-muted">{tasks.length}</span>
          </div>
          <p className="mt-0.5 truncate text-xs text-text-muted">{t("queueCenter.subtitle")}</p>
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-9 w-9"
              onClick={() => void refreshSnapshot()}
              disabled={snapshotLoading}
              aria-label={t("queueCenter.refreshSnapshot")}
            >
              <RefreshCcw className={`h-4 w-4 ${snapshotLoading ? "animate-spin" : ""}`} aria-hidden />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t("queueCenter.refreshSnapshot")}</TooltipContent>
        </Tooltip>
      </header>

      <CapacityStrip snapshot={snapshot} loading={snapshotLoading} queueCount={tasks.length} />

      {error ? (
        <div
          className="flex items-center gap-2 border-b border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger md:px-4"
          role="alert"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <Button variant="outline" size="sm" className="h-8" onClick={onRetryLoad}>
            <RefreshCcw className="h-3.5 w-3.5" aria-hidden />
            {t("queueCenter.retryLoad")}
          </Button>
        </div>
      ) : null}

      <div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] xl:grid-cols-[minmax(0,1fr)_15rem] xl:grid-rows-[minmax(0,1fr)]">
        <div className="order-2 min-h-0 min-w-0 overflow-y-auto xl:order-1">
          {loading && tasks.length === 0 ? (
            <QueueLoading label={t("queueCenter.loading")} />
          ) : tasks.length === 0 ? (
            <QueueEmpty
              title={t("queueCenter.emptyTitle")}
              description={t("queueCenter.emptyDescription")}
              action={t("queueCenter.viewAll")}
              onAction={() => setNav("all")}
            />
          ) : (
            <div className="pb-4">
              {groups.map((group) => (
                <section key={group.priority} aria-labelledby={`queue-priority-${group.priority}`}>
                  <div className="sticky top-0 z-10 flex h-8 items-center gap-2 border-b border-border-divider bg-surface-base/95 px-3 backdrop-blur-sm md:px-4">
                    <PriorityMark priority={group.priority} />
                    <h2 id={`queue-priority-${group.priority}`} className="text-xs font-semibold text-text-secondary">
                      {t(`queueCenter.priority.${group.priority}`)}
                    </h2>
                    <span className="font-mono text-xs text-text-muted">{group.tasks.length}</span>
                  </div>
                  {/* biome-ignore lint/a11y/useSemanticElements: Keyboard reorder handlers attach to a focusable grid of task rows; native ul/li would fight the existing layout and key handling. */}
                  <div
                    role="list"
                    aria-label={t(`queueCenter.priority.${group.priority}`)}
                    aria-controls={selectedTask ? `queue-details-${selectedTask.id}` : undefined}
                    onKeyDown={handleQueueListKeyDown}
                  >
                    {group.tasks.map((task) => (
                      <QueueTaskRow
                        key={task.id}
                        task={task}
                        position={tasks.findIndex((candidate) => candidate.id === task.id) + 1}
                        selected={selectedTask?.id === task.id}
                        decision={decisions.get(task.id) ?? null}
                        onSelect={() => focusTask(task.id)}
                        onPause={() => onPause(task)}
                        onMoveUp={() => onReorder?.(task, "move_up")}
                        onMoveDown={() => onReorder?.(task, "move_down")}
                      />
                    ))}
                  </div>
                </section>
              ))}
              {hasMore ? (
                <div className="flex justify-center border-t border-border-divider px-4 py-3">
                  <Button variant="ghost" size="sm" disabled={loading} onClick={onLoadMore}>
                    {loading ? t("queueCenter.loadingMore") : t("queueCenter.loadMore")}
                  </Button>
                </div>
              ) : null}
            </div>
          )}
        </div>

        <aside className="order-1 min-h-0 min-w-0 border-b border-border-divider bg-surface-base/55 xl:order-2 xl:border-b-0 xl:border-l">
          <details className="group max-h-full overflow-y-auto xl:hidden">
            <summary className="sticky top-0 z-10 flex min-h-11 cursor-pointer list-none items-center gap-2 bg-surface-base px-3 py-2 text-sm font-medium text-text-secondary md:px-4">
              <CircleGauge className="h-4 w-4 text-accent-primary" aria-hidden />
              <span className="flex-1">{t("queueCenter.schedulerDetails")}</span>
              {selectedTask ? (
                <span className="max-w-48 truncate text-xs text-text-muted">{selectedTask.fileName}</span>
              ) : null}
              <ChevronDown className="h-4 w-4 transition-transform duration-ui group-open:rotate-180" aria-hidden />
            </summary>
            <div className="border-t border-border-divider px-4 py-4">{schedulerDetails("queue-mobile")}</div>
          </details>
          <div className="hidden h-full min-h-0 overflow-y-auto px-4 py-4 xl:block">
            {schedulerDetails("queue-desktop")}
          </div>
        </aside>
      </div>
    </section>
  );
}

function CapacityStrip({
  snapshot,
  loading,
  queueCount,
}: {
  snapshot: SchedulerSnapshot | null;
  loading: boolean;
  queueCount: number;
}) {
  const { t } = useTranslation();
  const scheduleLabel = !snapshot?.scheduleWindowEnabled
    ? t("queueCenter.scheduleOff")
    : snapshot.scheduleWindowActive
      ? t("queueCenter.scheduleOpenUntil", { time: snapshot.scheduleWindowEnd })
      : t("queueCenter.scheduleClosedUntil", { time: snapshot.scheduleWindowStart });
  return (
    <section
      className="grid min-h-11 grid-cols-2 border-b border-border-divider bg-surface-base/70 sm:grid-cols-3"
      aria-label={t("queueCenter.capacityLabel")}
    >
      <CapacityItem
        label={t("queueCenter.activeSlots")}
        value={loading && !snapshot ? "--" : `${snapshot?.activeTaskCount ?? 0}/${snapshot?.maxActiveTasks ?? 0}`}
        tone="energy"
      />
      <CapacityItem label={t("queueCenter.waitingTasks")} value={String(queueCount)} />
      <CapacityItem
        label={t("queueCenter.downloadWindow")}
        value={scheduleLabel}
        className="col-span-2 sm:col-span-1"
      />
    </section>
  );
}

function CapacityItem({
  label,
  value,
  tone = "default",
  className = "",
}: {
  label: string;
  value: string;
  tone?: "default" | "energy";
  className?: string;
}) {
  return (
    <div
      className={`flex min-w-0 items-center gap-2 border-r border-border-divider px-3 py-2 last:border-r-0 md:px-4 ${className}`}
    >
      <span className="shrink-0 text-xs text-text-muted">{label}</span>
      <span
        className={`ml-auto truncate font-mono text-sm font-semibold ${tone === "energy" ? "text-accent-energy" : "text-text-primary"}`}
      >
        {value}
      </span>
    </div>
  );
}

function QueueTaskRow({
  task,
  position,
  selected,
  decision,
  onSelect,
  onPause,
  onMoveUp,
  onMoveDown,
}: {
  task: Task;
  position: number;
  selected: boolean;
  decision: QueueTaskDecision | null;
  onSelect: () => void;
  onPause: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const { t } = useTranslation();
  return (
    // biome-ignore lint/a11y/useSemanticElements: Rows are interactive grid cells with nested buttons; listitem role preserves the parent list semantics without invalid li nesting.
    <div
      id={`queue-task-${task.id}`}
      role="listitem"
      aria-current={selected ? "true" : undefined}
      tabIndex={selected ? 0 : -1}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      className={`grid min-h-16 min-w-0 grid-cols-[2.75rem_minmax(0,1fr)_8rem_5rem] items-center border-b border-border-divider px-2 transition-colors duration-ui focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent-primary sm:min-h-12 sm:grid-cols-[3rem_minmax(10rem,1fr)_minmax(7rem,0.45fr)_9rem_5rem] sm:px-3 ${
        selected ? "bg-accent-primary/10" : "hover:bg-surface-hover"
      }`}
    >
      <span className="font-mono text-xs text-text-muted">#{position}</span>
      <span className="min-w-0 pr-3">
        <span className="block truncate text-sm font-medium leading-5 text-text-primary">{task.fileName}</span>
        <span className="block truncate text-xs leading-4 text-text-muted sm:hidden">
          {task.sourceKey} · {formatBytes(task.totalSize)}
        </span>
      </span>
      <span className="hidden min-w-0 pr-3 sm:block">
        <span className="block truncate text-xs text-text-secondary">{task.sourceKey}</span>
        <span className="block font-mono text-xs text-text-muted">{formatBytes(task.totalSize)}</span>
      </span>
      <QueueReason decision={decision} retryAfterAt={task.retryAfterAt} />
      <span className="flex justify-end gap-0.5" aria-controls={`queue-task-${task.id}`}>
        <QueueIconButton label={t("queueCenter.moveUp")} onClick={onMoveUp} tabIndex={-1}>
          <ArrowUp className="h-3.5 w-3.5" aria-hidden />
        </QueueIconButton>
        <QueueIconButton label={t("queueCenter.moveDown")} onClick={onMoveDown} tabIndex={-1}>
          <ArrowDown className="h-3.5 w-3.5" aria-hidden />
        </QueueIconButton>
        <QueueIconButton
          label={t("queueCenter.pauseTask")}
          onClick={onPause}
          className="hidden sm:inline-flex"
          tabIndex={-1}
        >
          <Pause className="h-3.5 w-3.5" aria-hidden />
        </QueueIconButton>
      </span>
    </div>
  );
}

function QueueReason({ decision, retryAfterAt }: { decision: QueueTaskDecision | null; retryAfterAt: string | null }) {
  const { t, i18n } = useTranslation();
  const reason = decision?.reason ?? "ready";
  const retryDate = retryAfterAt ? new Date(retryAfterAt) : null;
  const retryTime =
    retryDate && !Number.isNaN(retryDate.getTime())
      ? new Intl.DateTimeFormat(i18n.language, { hour: "2-digit", minute: "2-digit" }).format(retryDate)
      : "";
  const label = t(`queueCenter.reason.${reason}`, { time: retryTime });
  const tone =
    queueReasonTone(reason) === "ready"
      ? "text-accent-energy"
      : queueReasonTone(reason) === "muted"
        ? "text-text-muted"
        : "text-status-warning";
  return (
    <span className={`min-w-0 truncate pr-2 text-xs font-medium ${tone}`} title={label}>
      {label}
    </span>
  );
}

function SchedulerDetails({
  idPrefix,
  snapshot,
  snapshotError,
  snapshotLoading,
  selectedTask,
  selectedDecision,
  updating,
  onRefresh,
  onUpdateOptions,
  onReorder,
  onPause,
  onShowDetails,
}: {
  idPrefix: string;
  snapshot: SchedulerSnapshot | null;
  snapshotError: string | null;
  snapshotLoading: boolean;
  selectedTask: Task | null;
  selectedDecision: QueueTaskDecision | null;
  updating: boolean;
  onRefresh: () => Promise<void>;
  onUpdateOptions: (task: Task, patch: { priority?: TaskPriority; obeySchedule?: boolean }) => Promise<void>;
  onReorder?: (task: Task, action: ReorderAction) => void;
  onPause: (task: Task) => void;
  onShowDetails?: (task: Task) => void;
}) {
  const { t } = useTranslation();
  if (snapshotError && !snapshot) {
    return (
      <div role="alert" className="space-y-3 text-sm text-status-danger">
        <p>{snapshotError}</p>
        <Button variant="outline" size="sm" onClick={() => void onRefresh()}>
          <RefreshCcw className="h-3.5 w-3.5" aria-hidden />
          {t("queueCenter.retrySnapshot")}
        </Button>
      </div>
    );
  }
  return (
    <div className="grid gap-5 sm:grid-cols-2 xl:grid-cols-1">
      <section
        className="min-w-0"
        id={selectedTask ? `queue-details-${selectedTask.id}` : undefined}
        aria-labelledby={`${idPrefix}-selected-title`}
      >
        <h2 id={`${idPrefix}-selected-title`} className="text-xs font-medium text-text-muted">
          {t("queueCenter.selectedTask")}
        </h2>
        {selectedTask ? (
          <div className="mt-2 space-y-3">
            <div className="min-w-0">
              <p className="truncate text-sm font-semibold text-text-primary">{selectedTask.fileName}</p>
              <QueueReason decision={selectedDecision} retryAfterAt={selectedTask.retryAfterAt} />
            </div>
            <div className="block space-y-1">
              <span
                id={`${idPrefix}-priority-label-${selectedTask.id}`}
                className="text-xs font-medium text-text-secondary"
              >
                {t("queueCenter.priorityLabel")}
              </span>
              <Select
                value={selectedTask.priority}
                onValueChange={(value) => void onUpdateOptions(selectedTask, { priority: value as TaskPriority })}
                disabled={updating}
              >
                <SelectTrigger
                  className="h-9 w-full bg-surface-root"
                  aria-labelledby={`${idPrefix}-priority-label-${selectedTask.id}`}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PRIORITY_ORDER.map((priority) => (
                    <SelectItem key={priority} value={priority}>
                      {t(`queueCenter.priority.${priority}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center justify-between gap-3">
              <span className="text-xs font-medium text-text-secondary">{t("queueCenter.obeySchedule")}</span>
              <Switch
                checked={selectedTask.obeySchedule}
                onCheckedChange={(checked) => void onUpdateOptions(selectedTask, { obeySchedule: checked })}
                disabled={updating}
                aria-label={t("queueCenter.obeySchedule")}
              />
            </div>
            <fieldset className="m-0 grid min-w-0 grid-cols-4 gap-1 border-0 p-0">
              <legend className="sr-only">{t("queueCenter.reorderControls")}</legend>
              <QueueIconButton
                label={t("queueCenter.moveTop")}
                onClick={() => onReorder?.(selectedTask, "move_to_top")}
              >
                <ChevronsUp className="h-4 w-4" aria-hidden />
              </QueueIconButton>
              <QueueIconButton label={t("queueCenter.moveUp")} onClick={() => onReorder?.(selectedTask, "move_up")}>
                <ArrowUp className="h-4 w-4" aria-hidden />
              </QueueIconButton>
              <QueueIconButton label={t("queueCenter.moveDown")} onClick={() => onReorder?.(selectedTask, "move_down")}>
                <ArrowDown className="h-4 w-4" aria-hidden />
              </QueueIconButton>
              <QueueIconButton
                label={t("queueCenter.moveBottom")}
                onClick={() => onReorder?.(selectedTask, "move_to_bottom")}
              >
                <ChevronsDown className="h-4 w-4" aria-hidden />
              </QueueIconButton>
            </fieldset>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size="sm" onClick={() => onPause(selectedTask)}>
                <Pause className="h-3.5 w-3.5" aria-hidden />
                {t("queueCenter.pauseTask")}
              </Button>
              {onShowDetails ? (
                <Button variant="ghost" size="sm" onClick={() => onShowDetails(selectedTask)}>
                  <ExternalLink className="h-3.5 w-3.5" aria-hidden />
                  {t("queueCenter.fullDetails")}
                </Button>
              ) : null}
            </div>
          </div>
        ) : (
          <p className="mt-2 text-sm leading-5 text-text-muted">{t("queueCenter.noSelection")}</p>
        )}
      </section>

      <section
        className="min-w-0 border-t border-border-divider pt-4 sm:border-t-0 sm:pt-0 xl:border-t xl:pt-4"
        aria-labelledby={`${idPrefix}-scheduler-title`}
      >
        <div className="flex items-center justify-between gap-2">
          <h2 id={`${idPrefix}-scheduler-title`} className="text-xs font-medium text-text-muted">
            {t("queueCenter.schedulerState")}
          </h2>
          {snapshotLoading ? <RefreshCcw className="h-3.5 w-3.5 animate-spin text-text-muted" aria-hidden /> : null}
        </div>
        {snapshot ? (
          <div className="mt-2 space-y-3">
            <SchedulerValue
              icon={CircleGauge}
              label={t("queueCenter.taskSlots")}
              value={`${snapshot.activeTaskCount}/${snapshot.maxActiveTasks}`}
            />
            <SchedulerValue
              icon={Clock3}
              label={t("queueCenter.schedule")}
              value={
                !snapshot.scheduleWindowEnabled
                  ? t("queueCenter.scheduleOff")
                  : snapshot.scheduleWindowActive
                    ? t("queueCenter.scheduleOpen")
                    : t("queueCenter.scheduleClosed")
              }
            />
            <div className="border-t border-border-divider pt-3">
              <div className="mb-2 flex items-center gap-2 text-xs font-medium text-text-secondary">
                <Server className="h-3.5 w-3.5" aria-hidden />
                {t("queueCenter.hostConnections")}
              </div>
              {snapshot.hosts.length > 0 ? (
                <div className="space-y-2">
                  {snapshot.hosts.map((host) => (
                    <div key={host.sourceKey} className="min-w-0">
                      <div className="flex items-center gap-2 text-xs">
                        <span className="min-w-0 flex-1 truncate text-text-muted">{host.sourceKey}</span>
                        <span className="font-mono text-text-secondary">
                          {host.usedSlots}/{host.limit}
                        </span>
                      </div>
                      <div className="mt-1 h-1 overflow-hidden rounded-full bg-surface-track">
                        <div
                          className="h-full rounded-full bg-accent-energy"
                          style={{ width: `${Math.min(100, (host.usedSlots / Math.max(1, host.limit)) * 100)}%` }}
                        />
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-xs leading-5 text-text-muted">{t("queueCenter.noActiveHosts")}</p>
              )}
            </div>
          </div>
        ) : null}
      </section>
    </div>
  );
}

function SchedulerValue({
  icon: Icon,
  label,
  value,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <Icon className="h-3.5 w-3.5 text-accent-primary" />
      <span className="min-w-0 flex-1 truncate text-xs text-text-muted">{label}</span>
      <span className="font-mono text-sm font-semibold text-text-primary">{value}</span>
    </div>
  );
}

function QueueIconButton({
  label,
  onClick,
  children,
  className = "",
  tabIndex,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
  className?: string;
  tabIndex?: number;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className={`h-8 w-8 p-0 ${className}`}
          tabIndex={tabIndex}
          onClick={(event) => {
            event.stopPropagation();
            onClick();
          }}
          aria-label={label}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function PriorityMark({ priority }: { priority: TaskPriority }) {
  return (
    <span
      className={`h-2 w-2 rounded-full ${
        priority === "high" ? "bg-status-warning" : priority === "normal" ? "bg-accent-primary" : "bg-text-muted"
      }`}
      aria-hidden
    />
  );
}

function QueueLoading({ label }: { label: string }) {
  return (
    <div className="space-y-px" role="status" aria-label={label}>
      {Array.from({ length: 8 }, (_, index) => (
        <div key={index} className="flex min-h-12 items-center gap-3 border-b border-border-divider px-4">
          <span className="h-3 w-8 animate-pulse rounded bg-surface-raised" />
          <span className="h-3 flex-1 animate-pulse rounded bg-surface-raised" />
          <span className="h-3 w-24 animate-pulse rounded bg-surface-raised/70" />
        </div>
      ))}
    </div>
  );
}

function QueueEmpty({
  title,
  description,
  action,
  onAction,
}: {
  title: string;
  description: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <div className="flex min-h-64 flex-col items-center justify-center px-6 py-12 text-center">
      <span className="flex h-11 w-11 items-center justify-center rounded-lg bg-accent-energy/10 text-accent-energy">
        <CheckCircle2 className="h-5 w-5" aria-hidden />
      </span>
      <h2 className="mt-3 text-sm font-semibold text-text-primary">{title}</h2>
      <p className="mt-1 max-w-sm text-sm leading-5 text-text-muted">{description}</p>
      <Button variant="outline" size="sm" className="mt-4" onClick={onAction}>
        <ListOrdered className="h-3.5 w-3.5" aria-hidden />
        {action}
      </Button>
    </div>
  );
}

export function queueReasonTone(reason: QueueWaitReason): "ready" | "waiting" | "muted" {
  if (reason === "ready") return "ready";
  if (reason === "retry_delay") return "muted";
  return "waiting";
}
