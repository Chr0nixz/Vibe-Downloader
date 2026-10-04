import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import type { TaskRequestProfileView } from "@/generated/bindings";
import { localizedErrorMessage } from "@/lib/errors";
import { formatDateTime } from "@/lib/format-date";
import { EMPTY_REQUEST_PROFILE, parseRequestProfile, requestProfileDraft } from "@/lib/request-profile";
import { getTaskRequestProfile, updateTaskRequestProfile } from "@/lib/tauri";
import type { Task } from "@/types/task";
import { RequestProfileFields } from "./RequestProfileFields";

export function TaskRequestProfilePanel({ task }: { task: Task }) {
  const { t } = useTranslation();
  const translation = useRef(t);
  translation.current = t;
  const activeTaskId = useRef(task.id);
  activeTaskId.current = task.id;
  const [view, setView] = useState<TaskRequestProfileView | null>(null);
  const [draft, setDraft] = useState(EMPTY_REQUEST_PROFILE);
  const [replaceSensitive, setReplaceSensitive] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const editable = !["downloading", "retrying", "completed"].includes(task.status);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSaving(false);
    setView(null);
    setDraft(EMPTY_REQUEST_PROFILE);
    setSaved(false);
    void getTaskRequestProfile(task.id)
      .then((next) => {
        if (cancelled) return;
        setView(next);
        setDraft(requestProfileDraft(next));
        setReplaceSensitive(next.sensitiveExpired);
        setError(null);
      })
      .catch((err) => {
        if (!cancelled) setError(localizedErrorMessage(err, translation.current));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [task.id]);

  async function save() {
    const saveTaskId = task.id;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const input = parseRequestProfile(draft) ?? { userAgent: null, referer: null, customHeaders: [] };
      const next = await updateTaskRequestProfile(saveTaskId, input, replaceSensitive);
      if (activeTaskId.current !== saveTaskId) return;
      setView(next);
      setDraft(requestProfileDraft(next));
      setReplaceSensitive(false);
      setSaved(true);
    } catch (err) {
      if (activeTaskId.current === saveTaskId) setError(localizedErrorMessage(err, t));
    } finally {
      if (activeTaskId.current === saveTaskId) setSaving(false);
    }
  }
  return (
    <div className="space-y-2 border-t border-border-subtle pt-3">
      <RequestProfileFields
        idPrefix={`task-request-${task.id}`}
        value={draft}
        disabled={!editable || saving || loading}
        onChange={(next) => {
          setDraft(next);
          setSaved(false);
        }}
      />
      {view?.sensitiveHeaderNames.length ? (
        <div className="text-[11px] leading-4 text-text-muted">
          <p>{t("requestProfile.sensitiveSaved", { names: view.sensitiveHeaderNames.join(", ") })}</p>
          {view.sensitiveExpired ? (
            <p className="text-status-warning">{t("requestProfile.expired")}</p>
          ) : view.sensitiveExpiresAt ? (
            <p>{t("requestProfile.expires", { date: formatDateTime(view.sensitiveExpiresAt, "dateTime") })}</p>
          ) : null}
        </div>
      ) : null}
      <label
        htmlFor={`task-request-replace-${task.id}`}
        className="flex cursor-pointer items-start gap-2 text-xs text-text-secondary"
      >
        <Checkbox
          id={`task-request-replace-${task.id}`}
          checked={replaceSensitive}
          disabled={!editable || saving || loading}
          onChange={(event) => {
            setReplaceSensitive(event.target.checked);
            setSaved(false);
          }}
        />
        <span>
          {t("requestProfile.replaceSensitive")}
          <span className="block text-[11px] leading-4 text-text-muted">{t("requestProfile.replaceHint")}</span>
        </span>
      </label>
      {!editable ? <p className="text-[11px] text-text-muted">{t("errors.requestProfileActive")}</p> : null}
      {error ? (
        <p role="alert" className="text-xs text-status-danger">
          {error}
        </p>
      ) : null}
      <div className="flex items-center gap-2">
        <Button size="sm" variant="outline" onClick={() => void save()} disabled={!editable || loading || saving}>
          {t("requestProfile.save")}
        </Button>
        {saved ? (
          <span role="status" className="text-[11px] text-text-muted">
            {t("requestProfile.saved")}
          </span>
        ) : null}
      </div>
    </div>
  );
}
