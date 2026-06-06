import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { ProbeTaskPayload } from "@/generated/bindings";
import { createTask, openDirectoryPicker, probeTask } from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settings-store";
import { parseByteCount } from "@/types/task";
import type { Task } from "@/types/task";

export function NewDownloadDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (task: Task) => void;
}) {
  const { t } = useTranslation();
  const settings = useSettingsStore((s) => s.settings);
  const [url, setUrl] = useState("");
  const [saveDir, setSaveDir] = useState("");
  const [fileName, setFileName] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<ProbeTaskPayload | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function detect() {
    setProbing(true);
    setError(null);
    setProbe(null);
    try {
      const nextProbe = await probeTask({ url: url.trim() });
      setProbe(nextProbe);
      if (!fileName.trim()) {
        setFileName(nextProbe.fileName);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setProbing(false);
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const task = await createTask({
        url: url.trim(),
        saveDir: saveDir.trim() || null,
        fileName: fileName.trim() || null,
      });
      onCreated(task);
      setUrl("");
      setSaveDir("");
      setFileName("");
      setProbe(null);
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function chooseDirectory() {
    const selected = await openDirectoryPicker();
    if (selected) setSaveDir(selected);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("newDownload.title")}</DialogTitle>
        </DialogHeader>
        <form className="flex min-h-0 flex-1 flex-col overflow-hidden" onSubmit={submit}>
          <DialogBody className="flex flex-col gap-3 py-4">
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.url")}
              <Input
                value={url}
                onChange={(event) => {
                  setUrl(event.target.value);
                  setProbe(null);
                }}
                placeholder={t("newDownload.urlPlaceholder")}
                autoFocus
                required
              />
            </label>
            <div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={detect}
                disabled={probing || !url.trim()}
              >
                {probing ? t("newDownload.detecting") : t("newDownload.detect")}
              </Button>
            </div>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.saveDir")}
              <div className="flex flex-col gap-2 sm:flex-row">
                <Input
                  value={saveDir}
                  onChange={(event) => setSaveDir(event.target.value)}
                  placeholder={
                    settings?.defaultSaveDir ?? t("newDownload.saveDirPlaceholder")
                  }
                  className="h-10 sm:h-8"
                />
                <Button
                  type="button"
                  variant="outline"
                  className="h-10 shrink-0 sm:h-8"
                  onClick={chooseDirectory}
                  disabled={submitting}
                >
                  <FolderOpen className="h-4 w-4" />
                  {t("newDownload.chooseDirectory")}
                </Button>
              </div>
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("newDownload.fileName")}
              <Input
                value={fileName}
                onChange={(event) => setFileName(event.target.value)}
                placeholder={t("newDownload.fileNamePlaceholder")}
              />
            </label>
            {probe ? (
              <div className="grid grid-cols-1 gap-2 rounded-md border border-border-subtle bg-surface-raised/60 p-3 text-xs sm:grid-cols-2">
                <Info label={t("newDownload.probeFile")} value={probe.fileName} />
                <Info label={t("newDownload.probeSize")} value={formatBytes(parseByteCount(probe.totalSize))} />
                <Info label={t("newDownload.probeHost")} value={probe.sourceHost} />
                <Info
                  label={t("newDownload.probeResume")}
                  value={probe.supportsRange ? t("newDownload.resumeSupported") : t("newDownload.resumeUnavailable")}
                />
              </div>
            ) : null}
            {error ? (
              <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
                {error}
              </p>
            ) : null}
          </DialogBody>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              className="w-full sm:w-auto"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              {t("newDownload.cancel")}
            </Button>
            <Button type="submit" className="w-full sm:w-auto" disabled={submitting}>
              {submitting ? t("newDownload.starting") : t("newDownload.start")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-text-muted">{label}</div>
      <div className="truncate text-text-primary">{value}</div>
    </div>
  );
}
