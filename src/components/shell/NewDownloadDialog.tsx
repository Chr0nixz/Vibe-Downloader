import { useState, type FormEvent } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { ProbeTaskPayload } from "@/generated/bindings";
import { createTask, probeTask } from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New download</DialogTitle>
        </DialogHeader>
        <form className="flex flex-col gap-3 p-4" onSubmit={submit}>
          <label className="flex flex-col gap-1 text-xs text-text-muted">
            URL
            <Input
              value={url}
              onChange={(event) => {
                setUrl(event.target.value);
                setProbe(null);
              }}
              placeholder="https://example.com/file.zip"
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
              {probing ? "Detecting..." : "Detect"}
            </Button>
          </div>
          <label className="flex flex-col gap-1 text-xs text-text-muted">
            Save directory
            <Input
              value={saveDir}
              onChange={(event) => setSaveDir(event.target.value)}
              placeholder="Default Downloads folder"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-text-muted">
            File name
            <Input
              value={fileName}
              onChange={(event) => setFileName(event.target.value)}
              placeholder="Detected from the server"
            />
          </label>
          {probe ? (
            <div className="grid grid-cols-2 gap-2 rounded-md border border-border-subtle bg-surface-raised/60 p-3 text-xs">
              <Info label="File" value={probe.fileName} />
              <Info label="Size" value={formatBytes(parseByteCount(probe.totalSize))} />
              <Info label="Host" value={probe.sourceHost} />
              <Info
                label="Resume"
                value={probe.supportsRange ? "Supported" : "Unavailable"}
              />
            </div>
          ) : null}
          {error ? (
            <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-status-danger">
              {error}
            </p>
          ) : null}
          <div className="flex justify-end gap-2 pt-1">
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={submitting}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "Starting..." : "Start download"}
            </Button>
          </div>
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
