import * as ContextMenu from "@radix-ui/react-context-menu";
import {
  ArrowDown,
  ArrowUp,
  ChevronsDown,
  ChevronsUp,
  Clipboard,
  ClipboardCopy,
  ExternalLink,
  File,
  FileDown,
  FileText,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
  Square,
  Trash2,
} from "lucide-react";
import { memo } from "react";
import { useTranslation } from "react-i18next";

import { MenuContent, MenuItem, MenuLabel, MenuSeparator } from "@/components/ui/menu-item";
import type { RecoveryAction } from "@/generated/bindings";
import type { Task } from "@/types/task";

interface TaskContextMenuProps {
  task: Task;
  onToggleTransfer: (task: Task) => void;
  onRetry: (task: Task) => void;
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
  children: React.ReactNode;
}

export type ReorderAction = "move_to_top" | "move_up" | "move_down" | "move_to_bottom";

const showsResume = (status: Task["status"]) =>
  status === "paused" || status === "failed" || status === "waiting_network";

const hidesTransfer = (status: Task["status"]) => status === "completed" || status === "needs_attention";

export const TaskContextMenu = memo(function TaskContextMenu({
  task,
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onDelete,
  onDeleteFiles,
  onReorder,
  onCopyUrl,
  onCopyLocalPath,
  onShowDetails,
  children,
}: TaskContextMenuProps) {
  const { t } = useTranslation();
  const { status, protocol } = task;
  const canFinishRecording = protocol === "hls" && (status === "downloading" || status === "retrying");
  const canReorder = status === "queued" && onReorder;

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <MenuContent>
          {!hidesTransfer(status) && (
            <MenuItem
              icon={showsResume(status) ? Play : Pause}
              label={t(showsResume(status) ? "actions.resume" : "actions.pause")}
              onSelect={() => onToggleTransfer(task)}
            />
          )}

          {status === "failed" && (
            <MenuItem icon={RotateCcw} label={t("actions.retry")} onSelect={() => onRetry(task)} />
          )}

          {canFinishRecording && (
            <MenuItem icon={Square} label={t("actions.finishRecording")} onSelect={() => onFinishLiveRecording(task)} />
          )}

          {status === "completed" && (
            <MenuItem icon={File} label={t("actions.openFile")} onSelect={() => onOpenFile(task)} />
          )}

          <MenuItem icon={FolderOpen} label={t("actions.openFolder")} onSelect={() => onOpenFolder(task)} />

          {(onCopyUrl || onCopyLocalPath || onShowDetails) && (
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
          {onShowDetails && (
            <MenuItem
              icon={ExternalLink}
              label={t("contextmenu.task.showDetails")}
              onSelect={() => onShowDetails(task)}
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
