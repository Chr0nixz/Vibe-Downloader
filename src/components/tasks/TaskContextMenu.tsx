import * as ContextMenu from "@radix-ui/react-context-menu";
import { File, FolderOpen, Pause, Play, RotateCcw, Square, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

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
  onResolveAttention?: (task: Task, action: RecoveryAction) => void;
  children: React.ReactNode;
}

const showsResume = (status: Task["status"]) =>
  status === "paused" || status === "failed" || status === "waiting_network";

const hidesTransfer = (status: Task["status"]) => status === "completed" || status === "needs_attention";

export function TaskContextMenu({
  task,
  onToggleTransfer,
  onRetry,
  onFinishLiveRecording,
  onOpenFile,
  onOpenFolder,
  onDelete,
  children,
}: TaskContextMenuProps) {
  const { t } = useTranslation();
  const { status, protocol } = task;
  const canFinishRecording = protocol === "hls" && (status === "downloading" || status === "retrying");

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content
          className="min-w-44 overflow-hidden rounded-lg border border-border-panel bg-surface-overlay p-1 shadow-lg"
          alignOffset={4}
        >
          {!hidesTransfer(status) && (
            <Item
              icon={showsResume(status) ? Play : Pause}
              label={t(showsResume(status) ? "actions.resume" : "actions.pause")}
              onSelect={() => onToggleTransfer(task)}
            />
          )}

          {status === "failed" && <Item icon={RotateCcw} label={t("actions.retry")} onSelect={() => onRetry(task)} />}

          {canFinishRecording && (
            <Item icon={Square} label={t("actions.finishRecording")} onSelect={() => onFinishLiveRecording(task)} />
          )}

          {status === "completed" && (
            <Item icon={File} label={t("actions.openFile")} onSelect={() => onOpenFile(task)} />
          )}

          <Item icon={FolderOpen} label={t("actions.openFolder")} onSelect={() => onOpenFolder(task)} />

          <ContextMenu.Separator className="my-1 h-px bg-border-divider" />

          <Item icon={Trash2} label={t("deleteDialog.confirm")} destructive onSelect={() => onDelete(task)} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

/* ── List-area context menu (blank space) ── */

interface ListContextMenuProps {
  onNewDownload: () => void;
  children: React.ReactNode;
}

export function ListContextMenu({ onNewDownload, children }: ListContextMenuProps) {
  const { t } = useTranslation();

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="min-w-44 overflow-hidden rounded-lg border border-border-panel bg-surface-overlay p-1 shadow-lg">
          <Item label={t("palette.newDownload")} onSelect={onNewDownload} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

/* ── Internal item component ── */

function Item({
  icon: Icon,
  label,
  destructive,
  onSelect,
}: {
  icon?: React.ComponentType<{ className?: string }>;
  label: string;
  destructive?: boolean;
  onSelect: () => void;
}) {
  return (
    <ContextMenu.Item
      className={[
        "flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none",
        "transition-colors duration-[var(--motion-ui)] ease-out",
        "data-[highlighted]:bg-surface-raised",
        destructive
          ? "text-status-danger data-[highlighted]:text-status-danger"
          : "text-text-secondary data-[highlighted]:text-text-primary",
      ].join(" ")}
      onSelect={onSelect}
    >
      {Icon && <Icon className="h-4 w-4 shrink-0" />}
      <span className="truncate">{label}</span>
    </ContextMenu.Item>
  );
}
