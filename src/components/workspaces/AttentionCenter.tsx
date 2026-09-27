import { ArrowLeft, CheckCircle2, ExternalLink, LifeBuoy, RotateCcw } from "lucide-react";
import { type KeyboardEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";

import {
  ErrorCodeDisclosure,
  RecoveryConcernIcon,
  RecoveryProblem,
  TaskRecoveryActions,
} from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { RecoveryAction } from "@/generated/bindings";
import { localizedErrorMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import { sanitizeUrlForDisplay } from "@/lib/utils";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";
import { errorCodeForTask, RECOVERY_CONCERNS, type RecoveryConcern, recoveryConcern } from "./recovery-center-logic";

// Grouped by the Recovery Center's concern vocabulary rather than a taxonomy
// of its own: with two sets of names the same task was "Source" here and
// "Remote changed" one click away, and users read that as two problems.
type AttentionFilter = "all" | RecoveryConcern;

export function AttentionCenter({
  taskIds,
  loading,
  error,
  hasMore,
  onLoadMore,
  onRetryLoad,
  onResolve,
  onShowDetails,
}: {
  taskIds: string[];
  loading: boolean;
  error: string | null;
  hasMore: boolean;
  onLoadMore: () => void;
  onRetryLoad: () => void;
  onResolve: (task: Task, action: RecoveryAction) => void;
  onShowDetails?: (task: Task) => void;
}) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<AttentionFilter>("all");
  const [compactDetailOpen, setCompactDetailOpen] = useState(false);
  const selectedId = useTaskUIStore((state) => state.selectedId);
  const selectTask = useTaskUIStore((state) => state.selectTask);
  const setNav = useTaskUIStore((state) => state.setNav);
  const tasks = useTaskDataStore(
    useShallow((state) =>
      taskIds
        .map((id) => state.taskById[id])
        .filter((task): task is Task => Boolean(task) && task.status === "needs_attention"),
    ),
  );
  const selectedTask = tasks.find((task) => task.id === selectedId) ?? tasks[0] ?? null;

  useEffect(() => {
    if (!selectedTask) {
      setCompactDetailOpen(false);
      return;
    }
    if (selectedTask.id !== selectedId) selectTask(selectedTask.id);
  }, [selectTask, selectedId, selectedTask]);

  const grouped = useMemo(() => {
    const groups = new Map<RecoveryConcern, Task[]>();
    for (const concern of RECOVERY_CONCERNS) groups.set(concern, []);
    for (const task of tasks) groups.get(recoveryConcern(task))?.push(task);
    return groups;
  }, [tasks]);
  const visibleGroups = RECOVERY_CONCERNS.map((category) => ({
    category,
    tasks: grouped.get(category) ?? [],
  })).filter((group) => group.tasks.length > 0 && (filter === "all" || filter === group.category));
  // Flat order for roving tabindex across category sections (mirrors QueueCenter).
  const flatTasks = useMemo(() => visibleGroups.flatMap((group) => group.tasks), [visibleGroups]);
  const categoryCounts = Object.fromEntries(
    RECOVERY_CONCERNS.map((category) => [category, grouped.get(category)?.length ?? 0]),
  ) as Record<RecoveryConcern, number>;

  const chooseTask = (task: Task) => {
    selectTask(task.id);
    setCompactDetailOpen(true);
  };

  const focusTask = (taskId: string) => {
    const task = flatTasks.find((item) => item.id === taskId);
    if (!task) return;
    chooseTask(task);
    requestAnimationFrame(() => {
      document.getElementById(`attention-task-${taskId}`)?.focus();
    });
  };

  const handleListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (flatTasks.length === 0) return;
    const currentIndex = Math.max(
      0,
      flatTasks.findIndex((task) => task.id === selectedTask?.id),
    );
    let nextIndex = currentIndex;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      nextIndex = Math.min(flatTasks.length - 1, currentIndex + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      nextIndex = Math.max(0, currentIndex - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      nextIndex = 0;
    } else if (event.key === "End") {
      event.preventDefault();
      nextIndex = flatTasks.length - 1;
    } else {
      return;
    }
    const next = flatTasks[nextIndex];
    if (next) focusTask(next.id);
  };

  return (
    <section
      className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root"
      aria-labelledby="attention-title"
    >
      <header className="flex min-h-12 flex-wrap items-center gap-3 border-b border-border-divider px-3 py-2 md:px-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <h1 id="attention-title" className="text-base font-semibold leading-5 text-text-primary">
              {t("attentionCenter.title")}
            </h1>
            <span className="font-mono text-xs text-text-muted">{tasks.length}</span>
          </div>
          <p className="mt-0.5 truncate text-xs text-text-muted">{t("attentionCenter.subtitle")}</p>
        </div>

        <Tabs value={filter} onValueChange={(value) => setFilter(value as AttentionFilter)} className="hidden sm:block">
          <TabsList className="h-8 bg-surface-base p-0.5">
            <TabsTrigger value="all" className="h-7 px-2 text-xs">
              {t("attentionCenter.filterAll")}
            </TabsTrigger>
            {RECOVERY_CONCERNS.filter((category) => categoryCounts[category] > 0).map((category) => (
              <TabsTrigger key={category} value={category} className="h-7 gap-1 px-2 text-xs">
                {t(`recoveryCenter.concern.${category}`)}
                <span className="font-mono text-xs text-text-muted">{categoryCounts[category]}</span>
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        <Select value={filter} onValueChange={(value) => setFilter(value as AttentionFilter)}>
          <SelectTrigger className="h-9 w-36 bg-surface-base sm:hidden" aria-label={t("attentionCenter.filterLabel")}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("attentionCenter.filterAll")}</SelectItem>
            {RECOVERY_CONCERNS.filter((category) => categoryCounts[category] > 0).map((category) => (
              <SelectItem key={category} value={category}>
                {t(`recoveryCenter.concern.${category}`)} ({categoryCounts[category]})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {/* §3.3: Attention Center keeps the light entry; complex handling
            lives in the Recovery Center workspace. */}
        <Button variant="outline" size="sm" className="h-8" onClick={() => setNav("recovery")}>
          <LifeBuoy className="h-3.5 w-3.5" aria-hidden />
          {t("recoveryCenter.title")}
        </Button>
      </header>

      {error ? (
        <div
          className="flex items-center gap-2 border-b border-border-danger bg-status-danger/10 px-3 py-2 text-sm text-status-danger md:px-4"
          role="alert"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <Button variant="outline" size="sm" className="h-8" onClick={onRetryLoad}>
            <RotateCcw className="h-3.5 w-3.5" aria-hidden />
            {t("attentionCenter.retryLoad")}
          </Button>
        </div>
      ) : null}

      <div className="grid min-h-0 flex-1 lg:grid-cols-[minmax(19rem,42fr)_minmax(26rem,58fr)]">
        <div
          className={`${compactDetailOpen ? "hidden lg:flex" : "flex"} min-h-0 min-w-0 flex-col border-border-divider lg:border-r`}
        >
          <div className="min-h-0 flex-1 overflow-y-auto">
            {loading && tasks.length === 0 ? (
              <AttentionLoading label={t("attentionCenter.loading")} />
            ) : tasks.length === 0 ? (
              <AttentionEmpty
                title={t("attentionCenter.emptyTitle")}
                description={t("attentionCenter.emptyDescription")}
                action={t("attentionCenter.viewAll")}
                onAction={() => setNav("all")}
              />
            ) : visibleGroups.length === 0 ? (
              <AttentionEmpty
                title={t("attentionCenter.emptyFilterTitle")}
                description={t("attentionCenter.emptyFilterDescription")}
                action={t("attentionCenter.clearFilter")}
                onAction={() => setFilter("all")}
              />
            ) : (
              <div className="pb-4">
                {visibleGroups.map((group) => (
                  <section key={group.category} aria-labelledby={`attention-group-${group.category}`}>
                    <div className="sticky top-0 z-10 flex h-8 items-center gap-2 border-b border-border-divider bg-surface-base/95 px-3 backdrop-blur-sm md:px-4">
                      <RecoveryConcernIcon concern={group.category} className="h-3.5 w-3.5 text-status-warning" />
                      <h2
                        id={`attention-group-${group.category}`}
                        className="text-xs font-semibold text-text-secondary"
                      >
                        {t(`recoveryCenter.concern.${group.category}`)}
                      </h2>
                      <span className="font-mono text-xs text-text-muted">{group.tasks.length}</span>
                    </div>
                    {/* UX-18: use the same list/listitem semantics as TaskList and
                        QueueCenter (listbox/option here was the odd one out).
                        Arrow/Home/End navigation still lives on this container —
                        keydown bubbles up from the focused row, and
                        handleListKeyDown walks the flattened task list across
                        groups. */}
                    {/* biome-ignore lint/a11y/useSemanticElements: keep parity with the TaskList/QueueCenter list containers — a semantic <ul> would fight the grouped layout and key handling. */}
                    <div
                      role="list"
                      aria-label={t(`recoveryCenter.concern.${group.category}`)}
                      onKeyDown={handleListKeyDown}
                    >
                      {group.tasks.map((task) => (
                        <AttentionTaskRow
                          key={task.id}
                          task={task}
                          category={group.category}
                          selected={selectedTask?.id === task.id}
                          onSelect={() => chooseTask(task)}
                        />
                      ))}
                    </div>
                  </section>
                ))}
                {hasMore ? (
                  <div className="flex justify-center border-t border-border-divider px-4 py-3">
                    <Button variant="ghost" size="sm" disabled={loading} onClick={onLoadMore}>
                      {loading ? t("attentionCenter.loadingMore") : t("attentionCenter.loadMore")}
                    </Button>
                  </div>
                ) : null}
              </div>
            )}
          </div>
        </div>

        <div className={`${compactDetailOpen ? "flex" : "hidden lg:flex"} min-h-0 min-w-0 flex-col bg-surface-root`}>
          {selectedTask ? (
            <AttentionDetail
              task={selectedTask}
              onBack={() => {
                setCompactDetailOpen(false);
                requestAnimationFrame(() => document.getElementById(`attention-task-${selectedTask.id}`)?.focus());
              }}
              onResolve={onResolve}
              onShowDetails={onShowDetails}
            />
          ) : (
            <AttentionEmpty
              title={t("attentionCenter.selectTitle")}
              description={t("attentionCenter.selectDescription")}
            />
          )}
        </div>
      </div>
    </section>
  );
}

function AttentionTaskRow({
  task,
  category,
  selected,
  onSelect,
}: {
  task: Task;
  category: RecoveryConcern;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const updated = formatDateTime(task.updatedAt, "dateTime");
  return (
    /* biome-ignore lint/a11y/useSemanticElements: the row keeps the same listitem semantics as TaskList/QueueCenter rows — a semantic <li> would fight the grouped layout and key handling. */
    <div
      id={`attention-task-${task.id}`}
      role="listitem"
      aria-current={selected ? "true" : undefined}
      tabIndex={selected ? 0 : -1}
      onClick={onSelect}
      onKeyDown={(event) => {
        // Row is a div like TaskRow's: Enter/Space must activate the
        // selection the way the old <button> row did.
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      className={`grid min-h-16 w-full min-w-0 grid-cols-[1.75rem_minmax(0,1fr)_auto] items-center gap-2 border-b border-border-divider px-3 py-2 text-left transition-colors duration-ui focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent-primary md:px-4 lg:min-h-12 lg:py-1.5 ${
        selected ? "bg-accent-primary/12" : "hover:bg-surface-hover"
      }`}
    >
      <span className="flex h-7 w-7 items-center justify-center rounded-md bg-status-warning/12 text-status-warning">
        <RecoveryConcernIcon concern={category} className="h-4 w-4" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold leading-5 text-text-primary">{task.fileName}</span>
        <span className="block truncate text-xs leading-4 text-text-muted">
          {task.errorMessage ? localizedErrorMessage(task.errorMessage, t) : t("attentionCenter.actionRequired")}
        </span>
      </span>
      <span className="hidden min-w-20 text-right lg:block">
        <span className="block truncate text-xs text-text-secondary">{task.sourceKey}</span>
        <span className="block font-mono text-xs leading-4 text-text-muted">{updated}</span>
      </span>
    </div>
  );
}

function AttentionDetail({
  task,
  onBack,
  onResolve,
  onShowDetails,
}: {
  task: Task;
  onBack: () => void;
  onResolve: (task: Task, action: RecoveryAction) => void;
  onShowDetails?: (task: Task) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex min-h-12 items-center gap-2 border-b border-border-divider px-3 py-2 md:px-4">
        <Button
          variant="ghost"
          size="icon"
          className="h-9 w-9 lg:hidden"
          onClick={onBack}
          aria-label={t("attentionCenter.backToList")}
        >
          <ArrowLeft className="h-4 w-4" aria-hidden />
        </Button>
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-base font-semibold leading-5 text-text-primary">{task.fileName}</h2>
          <p className="truncate text-xs leading-4 text-text-muted">{sanitizeUrlForDisplay(task.url)}</p>
        </div>
        {onShowDetails ? (
          <Button variant="ghost" size="sm" className="h-9" onClick={() => onShowDetails(task)}>
            <ExternalLink className="h-3.5 w-3.5" aria-hidden />
            <span className="hidden sm:inline">{t("attentionCenter.fullDetails")}</span>
          </Button>
        ) : null}
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4 md:px-6 md:py-5">
        <div className="mx-auto max-w-3xl space-y-6">
          <section aria-labelledby="attention-problem-title">
            <RecoveryProblem
              task={task}
              headingId="attention-problem-title"
              fallbackMessage={t("attentionCenter.actionRequired")}
            />
          </section>

          <section className="border-t border-border-divider pt-5" aria-labelledby="attention-actions-title">
            <h3 id="attention-actions-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("attentionCenter.recommendedActions")}
            </h3>
            <TaskRecoveryActions task={task} onResolve={onResolve} showMessage={false} />
          </section>

          <section className="border-t border-border-divider pt-5" aria-labelledby="attention-context-title">
            <h3 id="attention-context-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("attentionCenter.context")}
            </h3>
            <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
              <DetailRow label={t("attentionCenter.source")} value={task.sourceKey} />
              <DetailRow label={t("attentionCenter.protocol")} value={task.protocol.toUpperCase()} mono />
              <DetailRow label={t("attentionCenter.saveDirectory")} value={task.saveDir} />
            </dl>
            <ErrorCodeDisclosure label={t("attentionCenter.errorCode")} code={errorCodeForTask(task)} />
          </section>
        </div>
      </div>
    </div>
  );
}

function DetailRow({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs leading-4 text-text-muted">{label}</dt>
      <dd className={`mt-0.5 break-words text-sm leading-5 text-text-secondary ${mono ? "font-mono text-xs" : ""}`}>
        {value}
      </dd>
    </div>
  );
}

function AttentionLoading({ label }: { label: string }) {
  return (
    <div className="space-y-px" role="status" aria-label={label}>
      {Array.from({ length: 7 }, (_, index) => (
        <div key={index} className="flex min-h-12 items-center gap-3 border-b border-border-divider px-4">
          <span className="h-7 w-7 animate-pulse rounded-md bg-surface-raised" />
          <span className="min-w-0 flex-1 space-y-1.5">
            <span className="block h-3 w-2/3 animate-pulse rounded bg-surface-raised" />
            <span className="block h-2.5 w-5/6 animate-pulse rounded bg-surface-raised/70" />
          </span>
        </div>
      ))}
    </div>
  );
}

function AttentionEmpty({
  title,
  description,
  action,
  onAction,
}: {
  title: string;
  description: string;
  action?: string;
  onAction?: () => void;
}) {
  return (
    <div className="flex min-h-64 flex-1 flex-col items-center justify-center px-6 py-12 text-center">
      <span className="flex h-11 w-11 items-center justify-center rounded-lg bg-status-success/10 text-status-success">
        <CheckCircle2 className="h-5 w-5" aria-hidden />
      </span>
      <h2 className="mt-3 text-sm font-semibold text-text-primary">{title}</h2>
      <p className="mt-1 max-w-sm text-sm leading-5 text-text-muted">{description}</p>
      {action && onAction ? (
        <Button variant="outline" size="sm" className="mt-4" onClick={onAction}>
          {action}
        </Button>
      ) : null}
    </div>
  );
}
