import type { TFunction } from "i18next";
import {
  AlertTriangle,
  ChevronRight,
  Clock,
  CloudOff,
  Copy,
  FilePenLine,
  FolderOpen,
  Globe,
  HardDrive,
  History,
  KeyRound,
  Link,
  Link2,
  type LucideIcon,
  PackageOpen,
  RotateCcw,
  Wrench,
} from "lucide-react";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { type RecoveryConcern, recoveryConcern } from "@/components/workspaces/recovery-center-logic";
import type { RecoveryAction } from "@/generated/bindings";
import {
  formatErrorForReport,
  localizedErrorCause,
  localizedErrorMessage,
  recoveryActionsForError,
} from "@/lib/errors";
import { createLogger } from "@/lib/logger";
import { cn, formatBytes } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";
import type { Task } from "@/types/task";

const log = createLogger("recovery");

/**
 * Failed is a hard stop; needs-attention is waiting on a decision. Every
 * surface that tints a stuck task takes its colour from here so the row, the
 * details panel, both centers and the sidebar badges tell the same story.
 */
export function recoveryTone(status: Task["status"]): "danger" | "warning" {
  return status === "failed" ? "danger" : "warning";
}

/**
 * What a restart throws away, in bytes the user can weigh. Null while nothing
 * is on disk yet, so callers keep the generic warning instead of promising a
 * loss of "0 B".
 */
export function restartCost(task: Pick<Task, "downloadedBytes" | "totalSize">, t: TFunction): string | null {
  if (task.downloadedBytes <= 0) return null;
  const downloaded = formatBytes(task.downloadedBytes);
  return task.totalSize > 0
    ? t("recovery.restartCost", { downloaded, total: formatBytes(task.totalSize) })
    : t("recovery.restartCostUnknownTotal", { downloaded });
}

export function TaskRecoveryActions({
  task,
  onResolve,
  showMessage = true,
}: {
  task: Task;
  onResolve: (task: Task, action: RecoveryAction) => void;
  /** Hosts that already state the problem above the actions (the Attention
   * Center detail pane) pass false, so the same sentence is not printed
   * twice in a row inside a tinted box. */
  showMessage?: boolean;
}) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const recoveryActions = task.recoveryActions ?? [];
  const actions = recoveryActions.length > 0 ? recoveryActions : recoveryActionsForError(task.errorMessage);

  const handleCopy = useCallback(async () => {
    if (!task.errorMessage) return;
    const text = formatErrorForReport(task.errorMessage, t, { taskId: task.id, url: task.url });
    // UX-24: report the clipboard outcome instead of toasting success
    // unconditionally — a swallowed rejection told the user "copied" while
    // the report never left the app.
    try {
      await navigator.clipboard.writeText(text);
      addToast({
        tone: "info",
        title: t("recovery.errorCopied"),
      });
    } catch (err) {
      log.warn("copy diagnostics failed", err);
      addToast({ tone: "error", title: t("contextmenu.task.copyFailed") });
    }
  }, [addToast, task.errorMessage, task.id, task.url, t]);

  if (!task.errorMessage || actions.length === 0) return null;

  const copyButton = (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="shrink-0 px-1.5"
      aria-label={t("recovery.copyError")}
      title={t("recovery.copyError")}
      onClick={(event) => {
        event.stopPropagation();
        handleCopy();
      }}
    >
      <Copy className="h-3.5 w-3.5" aria-hidden />
    </Button>
  );

  const actionButtons = actions.map((action) => (
    <Button
      key={action}
      type="button"
      variant={action === "restart" ? "danger" : "outline"}
      className="h-8"
      onClick={(event) => {
        event.stopPropagation();
        onResolve(task, action);
      }}
    >
      <RecoveryIcon action={action} />
      {t(`recovery.${action}`)}
    </Button>
  ));

  // The only destructive fix states its price next to the button, not only
  // inside the confirm dialog: the user should weigh it before choosing.
  const cost = actions.includes("restart") ? restartCost(task, t) : null;
  const costLine = cost ? <p className="mt-2 max-w-[65ch] text-xs leading-4 text-text-secondary">{cost}</p> : null;

  if (!showMessage) {
    return (
      <fieldset className="m-0 min-w-0 border-0 p-0">
        <legend className="sr-only">{t("recovery.groupLabel")}</legend>
        <div className="flex flex-wrap items-center gap-2">
          {actionButtons}
          {copyButton}
        </div>
        {costLine}
      </fieldset>
    );
  }

  const tone = recoveryTone(task.status);
  return (
    // No live region here: this block re-renders for every task the user
    // arrows through while details are open, and an alert per keystroke
    // drowned out the list itself. The row already announces status changes.
    <fieldset
      className={cn(
        "m-0 min-w-0 rounded-md border px-3 py-2.5",
        tone === "danger"
          ? "border-border-danger-subtle bg-status-danger/[0.06]"
          : "border-border-warning-subtle bg-status-warning/[0.06]",
      )}
    >
      <legend className="sr-only">{t("recovery.groupLabel")}</legend>
      <div className="flex items-start justify-between gap-2">
        <RecoveryProblem task={task} dense />
        {copyButton}
      </div>
      <div className="mt-2.5 flex flex-wrap gap-2">{actionButtons}</div>
      {costLine}
    </fieldset>
  );
}

/**
 * The problem statement every recovery surface opens with: the concern (same
 * vocabulary as the Recovery Center groups), the verdict, and the mechanism
 * behind it. The details panel, the Attention Center and the Recovery Center
 * all render this, so one failure reads the same wherever it is opened.
 */
export function RecoveryProblem({
  task,
  headingId,
  fallbackMessage,
  dense = false,
}: {
  task: Task;
  headingId?: string;
  /** Shown when the task carries no error payload at all. */
  fallbackMessage?: string;
  /** Narrow hosts (the 288px details panel) use the smaller type step. */
  dense?: boolean;
}) {
  const { t } = useTranslation();
  const concern = recoveryConcern(task);
  const tone = recoveryTone(task.status);
  const message = task.errorMessage ? localizedErrorMessage(task.errorMessage, t) : fallbackMessage;
  const cause = task.errorMessage ? localizedErrorCause(task.errorMessage, t) : undefined;
  return (
    <div className="flex min-w-0 items-start gap-2.5">
      <span
        className={cn(
          "flex shrink-0 items-center justify-center rounded-md",
          dense ? "h-6 w-6" : "mt-0.5 h-8 w-8",
          tone === "danger" ? "bg-status-danger/12 text-status-danger" : "bg-status-warning/12 text-status-warning",
        )}
      >
        <RecoveryConcernIcon concern={concern} className={dense ? "h-3.5 w-3.5" : "h-4 w-4"} />
      </span>
      <div className="min-w-0">
        <h3
          id={headingId}
          className={cn("font-semibold text-text-primary", dense ? "text-xs leading-6" : "text-sm leading-5")}
        >
          {t(`recoveryCenter.concern.${concern}`)}
        </h3>
        {message ? (
          <p className={cn("max-w-[65ch] text-text-secondary", dense ? "text-xs leading-5" : "mt-1 text-sm leading-5")}>
            {message}
          </p>
        ) : null}
        {cause ? <p className="mt-1 max-w-[65ch] text-xs leading-4 text-text-muted">{cause}</p> : null}
      </div>
    </div>
  );
}

/**
 * The raw stable error code, folded away. Support and bug reports need it, but
 * shown inline next to the source and folder it reads as the headline to
 * someone who only wants the fix.
 */
export function ErrorCodeDisclosure({ label, code }: { label: string; code: string | null }) {
  const { t } = useTranslation();
  if (!code) return null;
  return (
    <details className="group mt-4 text-xs text-text-muted">
      <summary className="inline-flex cursor-default list-none items-center gap-1 rounded-sm py-0.5 hover:text-text-secondary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-primary [&::-webkit-details-marker]:hidden">
        <ChevronRight
          className="h-3 w-3 transition-transform duration-ui group-open:rotate-90 motion-reduce:transition-none"
          aria-hidden
        />
        {t("recovery.technicalDetails")}
      </summary>
      <dl className="mt-2 flex gap-2 pl-4">
        <dt>{label}</dt>
        <dd className="select-text font-mono text-text-secondary">{code}</dd>
      </dl>
    </details>
  );
}

export function RecoveryConcernIcon({ concern, className }: { concern: RecoveryConcern; className?: string }) {
  if (concern === "auth") return <KeyRound className={className} aria-hidden />;
  if (concern === "proxy") return <Globe className={className} aria-hidden />;
  if (concern === "disk") return <HardDrive className={className} aria-hidden />;
  if (concern === "remoteChanged") return <Link2 className={className} aria-hidden />;
  if (concern === "resume") return <PackageOpen className={className} aria-hidden />;
  if (concern === "protocol") return <CloudOff className={className} aria-hidden />;
  if (concern === "http") return <History className={className} aria-hidden />;
  return <AlertTriangle className={className} aria-hidden />;
}

/** One glyph per recovery action, shared by the details panel, the row's
 * context menu and the palette so a fix looks the same wherever it is offered. */
export function recoveryActionIcon(action: RecoveryAction): LucideIcon {
  switch (action) {
    case "choose_another_name":
      return FilePenLine;
    case "choose_another_folder":
    case "open_folder":
      return FolderOpen;
    case "free_disk_space":
      return HardDrive;
    case "check_url":
      return Link;
    case "configure_ffmpeg":
    case "manage_sftp_host_keys":
      return Wrench;
    case "retry_later":
      return Clock;
    default:
      return RotateCcw;
  }
}

function RecoveryIcon({ action }: { action: RecoveryAction }) {
  const Icon = recoveryActionIcon(action);
  return <Icon className="h-4 w-4" aria-hidden />;
}
