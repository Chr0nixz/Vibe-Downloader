import type { TFunction } from "i18next";
import {
  ArrowLeft,
  CheckCircle2,
  ExternalLink,
  History,
  KeyRound,
  Loader2,
  RotateCcw,
  ShieldCheck,
} from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";

import {
  ErrorCodeDisclosure,
  RecoveryConcernIcon,
  RecoveryProblem,
  recoveryTone,
} from "@/components/tasks/TaskRecoveryActions";
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
import type { RecoveryAction, RecoveryHistoryRecord } from "@/generated/bindings";
import { errorMessage, localizedErrorMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import { bulkResolveAttention, listRecoveryHistory, updateTaskCredentials } from "@/lib/tauri";
import { formatBytes, sanitizeUrlForDisplay } from "@/lib/utils";
import { useTaskDataStore, useTaskUIStore } from "@/stores/task-store";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";
import {
  type ConcernGroup,
  errorCodeForTask,
  groupRecoveryConcerns,
  HISTORY_ACTION_KEYS,
  HISTORY_SOURCE_KEYS,
  isAutoRecoverable,
  type PlaybookEntry,
  playbookForTask,
  type RecoveryConcern,
} from "./recovery-center-logic";

/** Protocols whose stored credentials the backend can replace (SEC parity
 * with commands/recovery.rs CREDENTIAL_PROTOCOLS). The HTTP family covers
 * Basic Auth on http/https plus the derived hls/dash/metalink engines;
 * bt/magnet have no credential channel. */
const CREDENTIAL_PROTOCOLS = new Set([
  "ftp",
  "ftps",
  "sftp",
  "webdav",
  "webdavs",
  "http",
  "https",
  "hls",
  "dash",
  "metalink",
]);

export function RecoveryCenter({
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
  const addToast = useToastStore((s) => s.addToast);
  const [showHistory, setShowHistory] = useState(false);
  const [history, setHistory] = useState<RecoveryHistoryRecord[] | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [busyConcern, setBusyConcern] = useState<RecoveryConcern | null>(null);
  const [credentialsTask, setCredentialsTask] = useState<Task | null>(null);
  const [compactDetailOpen, setCompactDetailOpen] = useState(false);
  const selectedId = useTaskUIStore((state) => state.selectedId);
  const selectTask = useTaskUIStore((state) => state.selectTask);
  const tasks = useTaskDataStore(
    useShallow((state) =>
      taskIds
        .map((id) => state.taskById[id])
        .filter(
          (task): task is Task => Boolean(task) && (task.status === "failed" || task.status === "needs_attention"),
        ),
    ),
  );
  const selectedTask = tasks.find((task) => task.id === selectedId) ?? tasks[0] ?? null;

  useEffect(() => {
    if (!selectedTask) setCompactDetailOpen(false);
  }, [selectedTask]);

  const refreshHistory = useCallback(async () => {
    setHistoryLoading(true);
    setHistoryError(null);
    try {
      setHistory(await listRecoveryHistory(30));
    } catch (err) {
      setHistoryError(errorMessage(err));
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    if (showHistory && history === null && !historyLoading) void refreshHistory();
  }, [history, historyLoading, refreshHistory, showHistory]);

  const groups = useMemo(() => groupRecoveryConcerns(tasks), [tasks]);
  const flatTasks = useMemo(() => groups.flatMap((group) => group.tasks), [groups]);

  const chooseTask = useCallback(
    (task: Task) => {
      selectTask(task.id);
      setCompactDetailOpen(true);
    },
    [selectTask],
  );

  const focusTask = (taskId: string) => {
    const task = flatTasks.find((item) => item.id === taskId);
    if (!task) return;
    chooseTask(task);
    requestAnimationFrame(() => {
      document.getElementById(`recovery-task-${taskId}`)?.focus();
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

  const runGroupRetry = async (group: ConcernGroup) => {
    const ids = group.tasks.filter(isAutoRecoverable).map((task) => task.id);
    if (ids.length === 0 || busyConcern) return;
    setBusyConcern(group.concern);
    try {
      const result = await bulkResolveAttention(ids, "retry");
      addToast({
        tone: result.failed > 0 ? "error" : "success",
        title: t("recoveryCenter.toast.bulkDone", {
          succeeded: result.succeeded,
          skipped: result.skipped,
          failed: result.failed,
        }),
        description: result.skipped > 0 ? t("recoveryCenter.toast.bulkSkippedHint") : undefined,
      });
      if (showHistory) void refreshHistory();
      else setHistory(null);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("recoveryCenter.toast.bulkFailed"),
        description: errorMessage(err),
      });
    } finally {
      setBusyConcern(null);
    }
  };

  const onCredentialsSaved = () => {
    setCredentialsTask(null);
    addToast({ tone: "success", title: t("recoveryCenter.credentials.toastSaved") });
    setHistory(null);
  };

  return (
    <section className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface-root" aria-labelledby="recovery-title">
      <header className="flex min-h-12 flex-wrap items-center gap-3 border-b border-border-divider px-3 py-2 md:px-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <h1 id="recovery-title" className="text-base font-semibold leading-5 text-text-primary">
              {t("recoveryCenter.title")}
            </h1>
            <span className="font-mono text-xs text-text-muted">{tasks.length}</span>
          </div>
          <p className="mt-0.5 truncate text-xs text-text-muted">{t("recoveryCenter.subtitle")}</p>
        </div>
        <Button
          variant={showHistory ? "default" : "outline"}
          size="sm"
          className="h-8"
          onClick={() => setShowHistory((value) => !value)}
          aria-pressed={showHistory}
        >
          <History className="h-3.5 w-3.5" aria-hidden />
          {t("recoveryCenter.history.title")}
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
            {t("recoveryCenter.retryLoad")}
          </Button>
        </div>
      ) : null}

      {showHistory ? (
        <RecoveryHistoryPanel
          history={history}
          loading={historyLoading}
          error={historyError}
          onRefresh={() => void refreshHistory()}
        />
      ) : (
        <div className="grid min-h-0 flex-1 lg:grid-cols-[minmax(19rem,42fr)_minmax(26rem,58fr)]">
          <div
            className={`${compactDetailOpen ? "hidden lg:flex" : "flex"} min-h-0 min-w-0 flex-col border-border-divider lg:border-r`}
          >
            <div className="min-h-0 flex-1 overflow-y-auto">
              {loading && tasks.length === 0 ? (
                <RecoveryLoading label={t("recoveryCenter.loading")} />
              ) : tasks.length === 0 ? (
                <RecoveryEmpty
                  title={t("recoveryCenter.emptyTitle")}
                  description={t("recoveryCenter.emptyDescription")}
                />
              ) : (
                <div className="pb-4">
                  {groups.map((group) => (
                    <section key={group.concern} aria-labelledby={`recovery-group-${group.concern}`}>
                      <div className="sticky top-0 z-10 flex h-9 items-center gap-2 border-b border-border-divider bg-surface-base/95 px-3 backdrop-blur-sm md:px-4">
                        <RecoveryConcernIcon concern={group.concern} className="h-3.5 w-3.5 text-status-warning" />
                        <h2
                          id={`recovery-group-${group.concern}`}
                          className="text-xs font-semibold text-text-secondary"
                        >
                          {t(`recoveryCenter.concern.${group.concern}`)}
                        </h2>
                        <span className="font-mono text-xs text-text-muted">{group.tasks.length}</span>
                        <span
                          className="ml-auto flex items-center gap-2 text-[0.6875rem] text-text-muted"
                          title={t("recoveryCenter.group.countsHint")}
                        >
                          <span className="rounded bg-status-success/10 px-1.5 py-0.5 font-mono text-status-success">
                            {t("recoveryCenter.group.autoCount", { count: group.autoRecoverable })}
                          </span>
                          <span className="rounded bg-surface-raised px-1.5 py-0.5 font-mono">
                            {t("recoveryCenter.group.needsInputCount", { count: group.needsInput })}
                          </span>
                        </span>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 px-2 text-xs"
                          disabled={group.autoRecoverable === 0 || busyConcern !== null}
                          onClick={() => void runGroupRetry(group)}
                        >
                          {busyConcern === group.concern ? (
                            <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
                          ) : (
                            <RotateCcw className="h-3 w-3" aria-hidden />
                          )}
                          {t("recoveryCenter.group.retryAll")}
                        </Button>
                      </div>
                      {/* UX-18: list/listitem semantics with container key
                          handling, same as TaskList/QueueCenter/AttentionCenter. */}
                      {/* biome-ignore lint/a11y/useSemanticElements: keep parity with the other workspace list containers. */}
                      <div
                        role="list"
                        aria-label={t(`recoveryCenter.concern.${group.concern}`)}
                        onKeyDown={handleListKeyDown}
                      >
                        {group.tasks.map((task) => (
                          <RecoveryTaskRow
                            key={task.id}
                            task={task}
                            concern={group.concern}
                            autoRecoverable={isAutoRecoverable(task)}
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
                        {loading ? t("recoveryCenter.loadingMore") : t("recoveryCenter.loadMore")}
                      </Button>
                    </div>
                  ) : null}
                </div>
              )}
            </div>
          </div>

          <div className={`${compactDetailOpen ? "flex" : "hidden lg:flex"} min-h-0 min-w-0 flex-col bg-surface-root`}>
            {selectedTask ? (
              <RecoveryDetail
                task={selectedTask}
                onBack={() => {
                  setCompactDetailOpen(false);
                  requestAnimationFrame(() => document.getElementById(`recovery-task-${selectedTask.id}`)?.focus());
                }}
                onResolve={onResolve}
                onShowDetails={onShowDetails}
                onUpdateCredentials={() => setCredentialsTask(selectedTask)}
              />
            ) : (
              <RecoveryEmpty
                title={t("recoveryCenter.selectTitle")}
                description={t("recoveryCenter.selectDescription")}
              />
            )}
          </div>
        </div>
      )}

      {/* Keyed by task id (or closed) so a fresh mount clears the plaintext
          fields on every open and unmounting drops them from memory. */}
      <CredentialsDialog
        key={credentialsTask?.id ?? "closed"}
        task={credentialsTask}
        onOpenChange={(open) => {
          if (!open) setCredentialsTask(null);
        }}
        onSaved={onCredentialsSaved}
      />
    </section>
  );
}

function RecoveryTaskRow({
  task,
  concern,
  autoRecoverable,
  selected,
  onSelect,
}: {
  task: Task;
  concern: RecoveryConcern;
  autoRecoverable: boolean;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const updated = formatDateTime(task.updatedAt, "dateTime");
  return (
    /* biome-ignore lint/a11y/useSemanticElements: the row keeps the same listitem semantics as the other workspace lists. */
    <div
      id={`recovery-task-${task.id}`}
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
      className={`grid min-h-16 w-full min-w-0 grid-cols-[1.75rem_minmax(0,1fr)_auto] items-center gap-2 border-b border-border-divider px-3 py-2 text-left transition-colors duration-ui focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent-primary md:px-4 lg:min-h-12 lg:py-1.5 ${
        selected ? "bg-accent-primary/12" : "hover:bg-surface-hover"
      }`}
    >
      <span
        className={`flex h-7 w-7 items-center justify-center rounded-md ${
          recoveryTone(task.status) === "danger"
            ? "bg-status-danger/12 text-status-danger"
            : "bg-status-warning/12 text-status-warning"
        }`}
      >
        <RecoveryConcernIcon concern={concern} className="h-4 w-4" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-sm font-semibold leading-5 text-text-primary">{task.fileName}</span>
        <span className="block truncate text-xs leading-4 text-text-muted">
          {task.errorMessage ? localizedErrorMessage(task.errorMessage, t) : t("recoveryCenter.actionRequired")}
        </span>
      </span>
      <span className="hidden min-w-20 text-right lg:block">
        <span className={`block truncate text-xs ${autoRecoverable ? "text-status-success" : "text-status-warning"}`}>
          {autoRecoverable ? t("recoveryCenter.row.auto") : t("recoveryCenter.row.needsInput")}
        </span>
        <span className="block font-mono text-xs leading-4 text-text-muted">{updated}</span>
      </span>
    </div>
  );
}

function RecoveryDetail({
  task,
  onBack,
  onResolve,
  onShowDetails,
  onUpdateCredentials,
}: {
  task: Task;
  onBack: () => void;
  onResolve: (task: Task, action: RecoveryAction) => void;
  onShowDetails?: (task: Task) => void;
  onUpdateCredentials: () => void;
}) {
  const { t } = useTranslation();
  const concern = groupRecoveryConcerns([task])[0]?.concern ?? "other";
  const playbook = playbookForTask(task);
  const credentialEligible = CREDENTIAL_PROTOCOLS.has(task.protocol);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex min-h-12 items-center gap-2 border-b border-border-divider px-3 py-2 md:px-4">
        <Button
          variant="ghost"
          size="icon"
          className="h-9 w-9 lg:hidden"
          onClick={onBack}
          aria-label={t("recoveryCenter.backToList")}
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
            <span className="hidden sm:inline">{t("recoveryCenter.fullDetails")}</span>
          </Button>
        ) : null}
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4 md:px-6 md:py-5">
        <div className="mx-auto max-w-3xl space-y-6">
          <section aria-labelledby="recovery-problem-title">
            <RecoveryProblem
              task={task}
              headingId="recovery-problem-title"
              fallbackMessage={t("recoveryCenter.actionRequired")}
            />
          </section>

          <section className="border-t border-border-divider pt-5" aria-labelledby="recovery-playbook-title">
            <h3 id="recovery-playbook-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("recoveryCenter.playbook.title")}
            </h3>
            {playbook.length === 0 ? (
              <p className="text-sm text-text-muted">{t("recoveryCenter.playbook.noActions")}</p>
            ) : (
              <ol className="space-y-3">
                {playbook.map((entry) => (
                  <li
                    key={entry.action}
                    className="rounded-lg border border-border-divider bg-surface-base/60 px-3 py-2.5"
                  >
                    <div className="flex items-center gap-2">
                      <span className="min-w-0 flex-1 text-sm font-semibold text-text-primary">
                        {t(`recovery.${entry.action}`)}
                      </span>
                      <Button
                        variant={entry.action === "restart" ? "danger" : "outline"}
                        size="sm"
                        className="h-7"
                        onClick={() => onResolve(task, entry.action)}
                      >
                        {t("recoveryCenter.playbook.run")}
                      </Button>
                    </div>
                    <dl className="mt-1.5 grid gap-x-6 gap-y-1 text-xs leading-4 text-text-muted sm:grid-cols-2">
                      <div className="flex gap-1.5">
                        <dt className="shrink-0">{t("recoveryCenter.playbook.keepsLabel")}</dt>
                        <dd className="min-w-0 text-text-secondary">{t(entry.keepsKey)}</dd>
                      </div>
                      <div className="flex gap-1.5">
                        <dt className="shrink-0">{t("recoveryCenter.playbook.deletesLabel")}</dt>
                        <dd className="min-w-0 text-text-secondary">{playbookDeletes(entry, task, t)}</dd>
                      </div>
                      <div className="flex gap-1.5">
                        <dt className="shrink-0">{t("recoveryCenter.playbook.redownloadsLabel")}</dt>
                        <dd className="text-text-secondary">{playbookRedownloads(entry, task, t)}</dd>
                      </div>
                      <div className="flex gap-1.5">
                        <dt className="shrink-0">{t("recoveryCenter.playbook.changesPathLabel")}</dt>
                        <dd className="text-text-secondary">
                          {entry.changesPath ? t("recoveryCenter.playbook.yes") : t("recoveryCenter.playbook.no")}
                        </dd>
                      </div>
                    </dl>
                  </li>
                ))}
              </ol>
            )}
          </section>

          {concern === "auth" && credentialEligible ? (
            <section className="border-t border-border-divider pt-5" aria-labelledby="recovery-credentials-title">
              <h3 id="recovery-credentials-title" className="mb-3 text-xs font-medium text-text-muted">
                {t("recoveryCenter.credentials.sectionTitle")}
              </h3>
              <div className="flex items-center gap-3 rounded-lg border border-border-divider bg-surface-base/60 px-3 py-2.5">
                <ShieldCheck className="h-4 w-4 shrink-0 text-status-success" aria-hidden />
                <p className="min-w-0 flex-1 text-xs leading-4 text-text-muted">
                  {t("recoveryCenter.credentials.hint")}
                </p>
                <Button variant="outline" size="sm" className="h-7" onClick={onUpdateCredentials}>
                  <KeyRound className="h-3 w-3" aria-hidden />
                  {t("recoveryCenter.credentials.open")}
                </Button>
              </div>
            </section>
          ) : null}

          <section className="border-t border-border-divider pt-5" aria-labelledby="recovery-context-title">
            <h3 id="recovery-context-title" className="mb-3 text-xs font-medium text-text-muted">
              {t("recoveryCenter.context")}
            </h3>
            <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
              <DetailRow label={t("recoveryCenter.source")} value={task.sourceKey} />
              <DetailRow label={t("recoveryCenter.protocol")} value={task.protocol.toUpperCase()} mono />
              <DetailRow label={t("recoveryCenter.saveDirectory")} value={task.saveDir} />
            </dl>
            <ErrorCodeDisclosure label={t("recoveryCenter.errorCode")} code={errorCodeForTask(task)} />
          </section>
        </div>
      </div>
    </div>
  );
}

function RecoveryHistoryPanel({
  history,
  loading,
  error,
  onRefresh,
}: {
  history: RecoveryHistoryRecord[] | null;
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="flex items-center gap-2 border-b border-border-divider px-3 py-2 md:px-4">
        <p className="min-w-0 flex-1 text-xs text-text-muted">{t("recoveryCenter.history.subtitle")}</p>
        <Button variant="outline" size="sm" className="h-8" disabled={loading} onClick={onRefresh}>
          <RotateCcw className="h-3.5 w-3.5" aria-hidden />
          {t("recoveryCenter.history.refresh")}
        </Button>
      </div>
      {error ? (
        <div className="px-4 py-3 text-sm text-status-danger" role="alert">
          {error}
        </div>
      ) : loading && history === null ? (
        <RecoveryLoading label={t("recoveryCenter.loading")} />
      ) : !history || history.length === 0 ? (
        <RecoveryEmpty
          title={t("recoveryCenter.history.emptyTitle")}
          description={t("recoveryCenter.history.emptyDescription")}
        />
      ) : (
        <ul className="pb-4">
          {history.map((entry) => (
            <li
              key={entry.id}
              className="grid min-h-11 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 border-b border-border-divider px-3 py-1.5 md:px-4"
            >
              <span className="min-w-0">
                <span className="block truncate text-sm leading-5 text-text-primary">
                  {entry.taskFileName ?? entry.taskId}
                </span>
                <span className="block truncate text-xs leading-4 text-text-muted">
                  {t(HISTORY_ACTION_KEYS[entry.action] ?? "recoveryCenter.history.action.unknown", {
                    defaultValue: entry.action,
                  })}
                  {" · "}
                  {t(HISTORY_SOURCE_KEYS[entry.source] ?? "recoveryCenter.history.source.unknown", {
                    defaultValue: entry.source,
                  })}
                  {entry.errorCode ? ` · ${entry.errorCode}` : ""}
                </span>
              </span>
              <span className="font-mono text-xs text-text-muted">{formatDateTime(entry.createdAt, "dateTime")}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CredentialsDialog({
  task,
  onOpenChange,
  onSaved,
}: {
  task: Task | null;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [privateKeyData, setPrivateKeyData] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);

  if (!task) return null;
  const isSftp = task.protocol === "sftp";

  const save = async () => {
    setBusy(true);
    try {
      await updateTaskCredentials({
        taskId: task.id,
        username,
        password,
        privateKeyData: isSftp && privateKeyData ? privateKeyData : null,
        privateKeyPassphrase: isSftp && passphrase ? passphrase : null,
      });
      onSaved();
    } catch (err) {
      addToast({
        tone: "error",
        title: t("recoveryCenter.credentials.toastFailed"),
        description: errorMessage(err),
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("recoveryCenter.credentials.title")}</DialogTitle>
          <DialogDescription>{t("recoveryCenter.credentials.description", { name: task.fileName })}</DialogDescription>
        </DialogHeader>
        <DialogBody className="space-y-3 py-4">
          <label htmlFor="recovery-cred-username" className="block space-y-1">
            <span className="text-xs font-medium text-text-secondary">{t("recoveryCenter.credentials.username")}</span>
            <Input
              id="recovery-cred-username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="off"
            />
          </label>
          <label htmlFor="recovery-cred-password" className="block space-y-1">
            <span className="text-xs font-medium text-text-secondary">{t("recoveryCenter.credentials.password")}</span>
            <Input
              id="recovery-cred-password"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="new-password"
            />
          </label>
          {isSftp ? (
            <>
              <label htmlFor="recovery-cred-private-key" className="block space-y-1">
                <span className="text-xs font-medium text-text-secondary">
                  {t("recoveryCenter.credentials.privateKey")}
                </span>
                <textarea
                  id="recovery-cred-private-key"
                  value={privateKeyData}
                  onChange={(event) => setPrivateKeyData(event.target.value)}
                  rows={4}
                  className="w-full rounded-md border border-border-divider bg-surface-base px-2.5 py-2 font-mono text-xs text-text-primary focus-visible:outline-2 focus-visible:outline-accent-primary"
                  spellCheck={false}
                />
              </label>
              <label htmlFor="recovery-cred-passphrase" className="block space-y-1">
                <span className="text-xs font-medium text-text-secondary">
                  {t("recoveryCenter.credentials.passphrase")}
                </span>
                <Input
                  id="recovery-cred-passphrase"
                  type="password"
                  value={passphrase}
                  onChange={(event) => setPassphrase(event.target.value)}
                  autoComplete="new-password"
                />
              </label>
            </>
          ) : null}
          <p className="text-xs leading-4 text-text-muted">{t("recoveryCenter.credentials.storageHint")}</p>
        </DialogBody>
        <DialogFooter>
          <Button variant="ghost" size="sm" disabled={busy} onClick={() => onOpenChange(false)}>
            {t("recoveryCenter.credentials.cancel")}
          </Button>
          <Button
            variant="default"
            size="sm"
            disabled={busy || (!username && !password && !privateKeyData)}
            onClick={() => void save()}
          >
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden /> : null}
            {t("recoveryCenter.credentials.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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

function RecoveryLoading({ label }: { label: string }) {
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

function RecoveryEmpty({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex min-h-64 flex-1 flex-col items-center justify-center px-6 py-12 text-center">
      <span className="flex h-11 w-11 items-center justify-center rounded-lg bg-status-success/10 text-status-success">
        <CheckCircle2 className="h-5 w-5" aria-hidden />
      </span>
      <h2 className="mt-3 text-sm font-semibold text-text-primary">{title}</h2>
      <p className="mt-1 max-w-sm text-sm leading-5 text-text-muted">{description}</p>
    </div>
  );
}

// The playbook's static wording ("all downloaded bytes") becomes a number
// once there is something on disk: "2.0 GB" is what the user is weighing.
function playbookDeletes(entry: PlaybookEntry, task: Task, t: TFunction): string {
  if (entry.action === "restart" && task.downloadedBytes > 0) {
    return t("recoveryCenter.playbook.restart.deletesAmount", { downloaded: formatBytes(task.downloadedBytes) });
  }
  return t(entry.deletesKey);
}

function playbookRedownloads(entry: PlaybookEntry, task: Task, t: TFunction): string {
  if (!entry.redownloads) return t("recoveryCenter.playbook.no");
  if (task.totalSize > 0) {
    return t("recoveryCenter.playbook.redownloadsAmount", { total: formatBytes(task.totalSize) });
  }
  return t("recoveryCenter.playbook.yes");
}
