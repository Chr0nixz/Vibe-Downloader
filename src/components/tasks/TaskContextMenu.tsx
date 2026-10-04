import * as ContextMenu from "@radix-ui/react-context-menu";
import {
  ArrowDown,
  ArrowUp,
  ChevronsDown,
  ChevronsUp,
  Clipboard,
  ClipboardCopy,
  File,
  FileDown,
  FileText,
  FolderOpen,
  PanelRight,
  Pause,
  Play,
  RotateCcw,
  Square,
  Trash2,
} from "lucide-react";
import { memo } from "react";
import { useTranslation } from "react-i18next";
import {
  inlineRecoveryActionsForTask,
  recoveryActionLabel,
  rowShowsRetry,
  rowTransferMode,
} from "@/components/tasks/row-recovery";
import { recoveryActionIcon } from "@/components/tasks/TaskRecoveryActions";
import { MenuContent, MenuItem, MenuLabel, MenuSeparator } from "@/components/ui/menu-item";
import type { RecoveryAction } from "@/generated/bindings";
import { formatBytes, formatShortcutForDocument } from "@/lib/utils";
import type { Task } from "@/types/task";

interface TaskContextMenuProps {
  task: Task;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
  onRedownload?: (task: Task) => void;
  onRecheck?: (task: Task) => void;
  onFinishLiveRecording: (task: Task) => void;
  onOpenFile: (task: Task) => void;
  onOpenFolder: (task: Task) => void;
  onDelete: (task: Task) => void;
  onDeleteFiles?: (task: Task) => void;
  onResolveAttention?: (task: Task, action: RecoveryAction) => void;
  onReorder?: (task: Task, action: ReorderAction) => void;
  onCopyUrl?: (task: Task) => void;
  onCopyLocalPath?: (task: Task) => void;
  onShowDetails?: (task: Task) => void;
  /** Select and focus the row before the menu's keyboard actions can run. */
  onContextMenu?: () => void;
  children: React.ReactNode;
}

export type ReorderAction = "move_to_top" | "move_up" | "move_down" | "move_to_bottom";

export const TaskContextMenu = memo(function TaskContextMenu({
  task,
  onToggleTransfer,
  onRetry,
  onRedownload,
  onRecheck,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onDelete,
  onDeleteFiles,
  onResolveAttention,
  onReorder,
  onCopyUrl,
  onCopyLocalPath,
  onShowDetails,
  onContextMenu,
  children,
}: TaskContextMenuProps) {
  const { t } = useTranslation();
  const { status, protocol } = task;
  const transferMode = rowTransferMode(task);
  const canFinishRecording = protocol === "hls" && (status === "downloading" || status === "retrying");
  const canReorder = status === "queued" && onReorder;
  // The row banner's fixes, offered here too: a restart-only failure used to
  // leave this menu with nothing but Open folder and Delete, so the one way to
  // recover it was a pointer click on the banner.
  const recoveryActions =
    onResolveAttention && (status === "failed" || status === "needs_attention")
      ? inlineRecoveryActionsForTask(task)
      : [];

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild onContextMenu={onContextMenu}>
        {children}
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <MenuContent>
          <MenuLabel>{task.fileName}</MenuLabel>
          <MenuSeparator />
          {recoveryActions.map((action) => (
            <MenuItem
              key={action}
              icon={recoveryActionIcon(action)}
              label={
                // The price of a restart is stated where it is chosen, as on the
                // banner and in the details panel.
                action === "restart" && task.downloadedBytes > 0
                  ? t("actions.restartDiscards", { size: formatBytes(task.downloadedBytes) })
                  : recoveryActionLabel(task, action, t)
              }
              destructive={action === "restart"}
              shortcut={action === recoveryActions[0] ? formatShortcutForDocument("mod+R") : undefined}
              onSelect={() => onResolveAttention?.(task, action)}
            />
          ))}
          {recoveryActions.length > 0 && <MenuSeparator />}

          {transferMode !== "hidden" && (
            <MenuItem
              icon={transferMode === "resume" ? Play : Pause}
              label={t(transferMode === "resume" ? "actions.resume" : "actions.pause")}
              onSelect={() => onToggleTransfer(task)}
            />
          )}

          {rowShowsRetry(task) && (
            <MenuItem
              icon={RotateCcw}
              label={t("actions.retry")}
              shortcut={formatShortcutForDocument("mod+R")}
              onSelect={() => onRetry(task)}
            />
          )}

          {canFinishRecording && (
            <MenuItem icon={Square} label={t("actions.finishRecording")} onSelect={() => onFinishLiveRecording(task)} />
          )}

          {status === "completed" && (
            <>
              {onRedownload ? (
                <MenuItem icon={RotateCcw} label={t("actions.redownload")} onSelect={() => onRedownload(task)} />
              ) : null}
              {onRecheck ? (
                <MenuItem icon={FileText} label={t("actions.recheck")} onSelect={() => onRecheck(task)} />
              ) : null}
              <MenuItem icon={File} label={t("actions.openFile")} onSelect={() => onOpenFile(task)} />
            </>
          )}

          <MenuItem icon={FolderOpen} label={t("actions.openFolder")} onSelect={() => onOpenFolder(task)} />

          {onShowDetails && (
            <MenuItem
              icon={PanelRight}
              label={t("contextmenu.task.showDetails")}
              onSelect={() => onShowDetails(task)}
            />
          )}

          {(onCopyUrl || onCopyLocalPath) && (
            <>
              <MenuSeparator />
              <MenuLabel>{t("contextmenu.task.section.copy")}</MenuLabel>
            </>
          )}

          {onCopyUrl && (
            <MenuItem icon={ClipboardCopy} label={t("contextmenu.task.copyUrl")} onSelect={() => onCopyUrl(task)} />
          )}
          {onCopyLocalPath && (
            <MenuItem
              icon={Clipboard}
              label={t("contextmenu.task.copyLocalPath")}
              onSelect={() => onCopyLocalPath(task)}
            />
          )}
          {canReorder && (
            <>
              <MenuSeparator />
              <MenuLabel>{t("contextmenu.task.section.queue")}</MenuLabel>
              <MenuItem
                icon={ChevronsUp}
                label={t("actions.moveToTop")}
                onSelect={() => onReorder?.(task, "move_to_top")}
              />
              <MenuItem icon={ArrowUp} label={t("actions.moveUp")} onSelect={() => onReorder?.(task, "move_up")} />
              <MenuItem
                icon={ArrowDown}
                label={t("actions.moveDown")}
                onSelect={() => onReorder?.(task, "move_down")}
              />
              <MenuItem
                icon={ChevronsDown}
                label={t("actions.moveToBottom")}
                onSelect={() => onReorder?.(task, "move_to_bottom")}
              />
            </>
          )}

          <MenuSeparator />

          <MenuItem
            icon={Trash2}
            label={t("deleteDialog.confirm")}
            shortcut="Del"
            destructive
            onSelect={() => onDelete(task)}
          />
          {onDeleteFiles ? (
            <MenuItem
              icon={Trash2}
              label={t("deleteDialog.deleteFilesToo")}
              shortcut="Shift+Del"
              destructive
              onSelect={() => onDeleteFiles(task)}
            />
          ) : null}
        </MenuContent>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
});

/* ── List-area context menu (blank space) ── */

interface ListContextMenuProps {
  onNewDownload: () => void;
  onPasteAndCreate?: () => void;
  onSelectAll?: () => void;
  onClearSelection?: () => void;
  onRefresh?: () => void;
  onExport?: (format: "json" | "csv") => void;
  hasSelection?: boolean;
  children: React.ReactNode;
}

export function ListContextMenu({
  onNewDownload,
  onPasteAndCreate,
  onSelectAll,
  onClearSelection,
  onRefresh,
  onExport,
  hasSelection,
  children,
}: ListContextMenuProps) {
  const { t } = useTranslation();

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <MenuContent>
          <MenuItem label={t("palette.newDownload")} onSelect={onNewDownload} />
          {onPasteAndCreate && <MenuItem label={t("contextmenu.list.pasteAndCreate")} onSelect={onPasteAndCreate} />}

          {(onSelectAll || onClearSelection) && <MenuSeparator />}

          {onSelectAll && <MenuItem label={t("contextmenu.list.selectAll")} onSelect={onSelectAll} />}
          {onClearSelection && (
            <MenuItem
              label={t("contextmenu.list.clearSelection")}
              disabled={!hasSelection}
              onSelect={onClearSelection}
            />
          )}

          {onRefresh && (
            <>
              <MenuSeparator />
              <MenuItem label={t("contextmenu.list.refresh")} onSelect={onRefresh} />
            </>
          )}

          {onExport && (
            <>
              <MenuSeparator />
              <MenuItem icon={FileDown} label={t("taskList.exportJson")} onSelect={() => onExport("json")} />
              <MenuItem icon={FileText} label={t("taskList.exportCsv")} onSelect={() => onExport("csv")} />
            </>
          )}
        </MenuContent>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
