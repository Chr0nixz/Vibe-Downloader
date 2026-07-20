import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Clock3,
  ExternalLink,
  FolderCog,
  Link2,
  RotateCcw,
  Settings2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";

import { TaskRecoveryActions } from "@/components/tasks/TaskRecoveryActions";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { RecoveryAction } from "@/generated/bindings";
import { localizedErrorMessage, parseAppError, recoveryActionsForError } from "@/lib/errors";
import { sanitizeUrlForDisplay } from "@/lib/utils";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import type { Task } from "@/types/task";

type AttentionCategory = "storage" | "source" | "runtime" | "retry" | "other";
type AttentionFilter = "all" | AttentionCategory;

const CATEGORY_ORDER: AttentionCategory[] = ["storage", "source", "runtime", "retry", "other"];

const STORAGE_ERROR_CODES = new Set([
  "disk_write_failed",
  "final_path_conflict",
  "temp_file_missing",
  "temp_file_smaller_than_progress",
]);
const SOURCE_ERROR_CODES = new Set([
  "auth_headers_expired",
  "http_denied",
  "http_not_found",
  "remote_changed",
  "resume_unavailable",
]);
const RETRY_ERROR_CODES = new Set(["server_rate_limited"]);
const STORAGE_ACTIONS = new Set<RecoveryAction>(["choose_another_name", "choose_another_folder", "free_disk_space"]);
const SOURCE_ACTIONS = new Set<RecoveryAction>(["check_url"]);
const RUNTIME_ACTIONS = new Set<RecoveryAction>(["configure_ffmpeg", "manage_sftp_host_keys", "restart"]);
const RETRY_ACTIONS = new Set<RecoveryAction>(["retry", "retry_later"]);

function actionsForTask(task: Task): RecoveryAction[] {
  return task.recoveryActions.length > 0 ? task.recoveryActions : recoveryActionsForError(task.errorMessage);
}

function errorCodeForTask(task: Task): string | null {
  return task.errorCode ?? parseAppError(task.errorMessage)?.code ?? null;
}

export function attentionCategory(task: Task): AttentionCategory {
  const errorCode = errorCodeForTask(task);
  if (errorCode && SOURCE_ERROR_CODES.has(errorCode)) return "source";
  if (errorCode && STORAGE_ERROR_CODES.has(errorCode)) return "storage";
  if (errorCode && RETRY_ERROR_CODES.has(errorCode)) return "retry";

  const actions = actionsForTask(task);
  if (actions.some((action) => SOURCE_ACTIONS.has(action))) return "source";
  if (actions.some((action) => STORAGE_ACTIONS.has(action))) return "storage";
  if (actions.some((action) => RUNTIME_ACTIONS.has(action))) return "runtime";
  if (actions.some((action) => RETRY_ACTIONS.has(action))) return "retry";
  return "other";
}

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
  const { t, i18n } = useTranslation();
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
    const groups = new Map<AttentionCategory, Task[]>();
    for (const category of CATEGORY_ORDER) groups.set(category, []);
    for (const task of tasks) groups.get(attentionCategory(task))?.push(task);
    return groups;
  }, [tasks]);
  const visibleGroups = CATEGORY_ORDER.map((category) => ({
    category,
    tasks: grouped.get(category) ?? [],
  })).filter((group) => group.tasks.length > 0 && (filter === "all" || filter === group.category));
  const categoryCounts = Object.fromEntries(
    CATEGORY_ORDER.map((category) => [category, grouped.get(category)?.length ?? 0]),
  ) as Record<AttentionCategory, number>;

  const chooseTask = (task: Task) => {
    selectTask(task.id);
    setCompactDetailOpen(true);
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
            {CATEGORY_ORDER.filter((category) => categoryCounts[category] > 0).map((category) => (
              <TabsTrigger key={category} value={category} className="h-7 gap-1 px-2 text-xs">
                {t(`attentionCenter.category.${category}`)}
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
            {CATEGORY_ORDER.filter((category) => categoryCounts[category] > 0).map((category) => (
              <SelectItem key={category} value={category}>
                {t(`attentionCenter.category.${category}`)} ({categoryCounts[category]})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
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
                      <CategoryIcon category={group.category} className="h-3.5 w-3.5 text-status-warning" />
                      <h2
                        id={`attention-group-${group.category}`}
                        className="text-xs font-semibold text-text-secondary"
                      >
                        {t(`attentionCenter.category.${group.category}`)}
                      </h2>
                      <span className="font-mono text-xs text-text-muted">{group.tasks.length}</span>
                    </div>
                    <div role="listbox" aria-label={t(`attentionCenter.category.${group.category}`)}>
                      {group.tasks.map((task) => (
                        <AttentionTaskRow
                          key={task.id}
                          task={task}
                          category={group.category}
                          selected={selectedTask?.id === task.id}
                          locale={i18n.language}
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
  locale,
  onSelect,
}: {
  task: Task;
  category: AttentionCategory;
  selected: boolean;
  locale: string;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const date = new Date(task.updatedAt);
  const updated = Number.isNaN(date.getTime())
    ? task.updatedAt
    : new Intl.DateTimeFormat(locale, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(
        date,
      );
  return (
    <button
      id={`attention-task-${task.id}`}
      type="button"
      role="option"
      aria-selected={selected}
      onClick={onSelect}
      className={`grid min-h-16 w-full min-w-0 grid-cols-[1.75rem_minmax(0,1fr)_auto] items-center gap-2 border-b border-border-divider px-3 py-2 text-left transition-colors duration-ui focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent-primary md:px-4 lg:min-h-12 lg:py-1.5 ${
        selected ? "bg-accent-primary/12" : "hover:bg-surface-hover"
      }`}
    >
      <span className="flex h-7 w-7 items-center justify-center rounded-md bg-status-warning/12 text-status-warning">
        <CategoryIcon category={category} className="h-4 w-4" />
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
    </button>
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
  const category = attentionCategory(task);
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
            <div className="flex items-start gap-3">
              <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-status-warning/12 text-status-warning">
                <CategoryIcon category={category} className="h-4 w-4" />
              </span>
              <div className="min-w-0">
                <h3 id="attention-problem-title" className="text-sm font-semibold leading-5 text-text-primary">
                  {t(`attentionCenter.category.${category}`)}
                </h3>
                <p className="mt-1 max-w-[65ch] text-sm leading-5 text-text-secondary">
                  {task.errorMessage
                    ? localizedErrorMessage(task.errorMessage, t)
                    : t("attentionCenter.actionRequired")}
                </p>
              </div>
            </div>
          </section>

          <section className="border-t border-border-divider pt-5" aria-labelledby="attention-actions-title">
            <h3 id="attention-actions-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("attentionCenter.recommendedActions")}
            </h3>
            <TaskRecoveryActions task={task} onResolve={onResolve} />
          </section>

          <section className="border-t border-border-divider pt-5" aria-labelledby="attention-context-title">
            <h3 id="attention-context-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("attentionCenter.context")}
            </h3>
            <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
              <DetailRow label={t("attentionCenter.source")} value={task.sourceKey} />
              <DetailRow label={t("attentionCenter.protocol")} value={task.protocol.toUpperCase()} mono />
              <DetailRow label={t("attentionCenter.saveDirectory")} value={task.saveDir} />
              <DetailRow
                label={t("attentionCenter.errorCode")}
                value={errorCodeForTask(task) ?? t("attentionCenter.notAvailable")}
                mono
              />
            </dl>
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

function CategoryIcon({ category, className }: { category: AttentionCategory; className?: string }) {
  if (category === "storage") return <FolderCog className={className} aria-hidden />;
  if (category === "source") return <Link2 className={className} aria-hidden />;
  if (category === "runtime") return <Settings2 className={className} aria-hidden />;
  if (category === "retry") return <Clock3 className={className} aria-hidden />;
  return <AlertTriangle className={className} aria-hidden />;
}
