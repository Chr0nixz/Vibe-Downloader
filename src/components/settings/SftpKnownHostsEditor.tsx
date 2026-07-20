import { KeyRound, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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
import type { SftpKnownHost } from "@/generated/bindings";
import { forgetSftpKnownHost, listSftpKnownHosts } from "@/lib/tauri";

interface SftpKnownHostsEditorProps {
  disabled?: boolean;
}

/**
 * ARC-15: manage TOFU SFTP host keys. Forget requires an explicit Dialog confirm
 * (no Undo toast) so a rotated server key cannot be accepted by accident.
 */
export function SftpKnownHostsEditor({ disabled }: SftpKnownHostsEditorProps) {
  const { t } = useTranslation();
  const [hosts, setHosts] = useState<SftpKnownHost[]>([]);
  const [loading, setLoading] = useState(true);
  const [pendingForget, setPendingForget] = useState<SftpKnownHost | null>(null);
  const [forgetting, setForgetting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setHosts(await listSftpKnownHosts());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function confirmForget() {
    if (!pendingForget) return;
    setForgetting(true);
    setError(null);
    try {
      await forgetSftpKnownHost(pendingForget.host, pendingForget.port);
      setPendingForget(null);
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setForgetting(false);
    }
  }

  return (
    <div className="grid gap-3" data-search-key="sftp_known_hosts">
      <div className="flex items-start gap-2">
        <KeyRound className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" aria-hidden />
        <div className="grid min-w-0 gap-1">
          <p className="text-sm font-medium text-text-primary">{t("settings.sftpKnownHosts")}</p>
          <p className="text-xs leading-5 text-text-muted">{t("settings.sftpKnownHostsTip")}</p>
        </div>
      </div>

      {error ? <p className="text-xs text-status-danger">{error}</p> : null}

      {loading ? (
        <p className="text-xs text-text-muted">{t("settings.sftpKnownHostsLoading")}</p>
      ) : hosts.length === 0 ? (
        <p className="text-xs text-text-muted">{t("settings.sftpKnownHostsEmpty")}</p>
      ) : (
        <ul className="divide-y divide-border-subtle rounded-md border border-border-subtle">
          {hosts.map((host) => (
            <li key={`${host.host}:${host.port}`} className="flex items-start justify-between gap-3 px-3 py-2.5">
              <div className="min-w-0 grid gap-0.5">
                <p className="truncate font-mono text-sm text-text-primary">
                  {host.host}:{host.port}
                </p>
                <p className="truncate text-xs text-text-muted">
                  {host.algorithm} · {host.fingerprintSha256}
                </p>
                <p className="text-[11px] text-text-muted">
                  {t("settings.sftpKnownHostsSeen", {
                    first: host.firstSeenAt,
                    last: host.lastSeenAt,
                  })}
                </p>
              </div>
              <Button
                type="button"
                variant="ghost"
                className="h-8 shrink-0"
                disabled={disabled || forgetting}
                onClick={() => setPendingForget(host)}
                aria-label={t("settings.sftpKnownHostsForget")}
              >
                <Trash2 className="h-4 w-4" />
                {t("settings.sftpKnownHostsForget")}
              </Button>
            </li>
          ))}
        </ul>
      )}

      <Dialog open={pendingForget != null} onOpenChange={(open) => !open && setPendingForget(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("settings.sftpKnownHostsForgetTitle")}</DialogTitle>
            <DialogDescription>
              {pendingForget
                ? t("settings.sftpKnownHostsForgetDescription", {
                    host: `${pendingForget.host}:${pendingForget.port}`,
                    fingerprint: pendingForget.fingerprintSha256,
                  })
                : null}
            </DialogDescription>
          </DialogHeader>
          <DialogBody />
          <DialogFooter>
            <Button type="button" variant="ghost" disabled={forgetting} onClick={() => setPendingForget(null)}>
              {t("recoveryDialog.cancel")}
            </Button>
            <Button type="button" variant="danger" disabled={forgetting} onClick={() => void confirmForget()}>
              {t("settings.sftpKnownHostsForgetConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
