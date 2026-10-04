import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import type { RequestProfileDraft } from "@/lib/request-profile";

export function RequestProfileFields({
  value,
  onChange,
  disabled = false,
  idPrefix,
}: {
  value: RequestProfileDraft;
  onChange: (value: RequestProfileDraft) => void;
  disabled?: boolean;
  idPrefix: string;
}) {
  const { t } = useTranslation();
  const hintId = `${idPrefix}-hint`;
  return (
    <div className="space-y-2">
      <span className="text-xs font-medium text-text-secondary">{t("requestProfile.title")}</span>
      <p id={hintId} className="text-[11px] leading-4 text-text-muted">
        {t("requestProfile.hint")}
      </p>
      <label htmlFor={`${idPrefix}-ua`} className="flex flex-col gap-1 text-xs text-text-muted">
        User-Agent
        <Input
          id={`${idPrefix}-ua`}
          value={value.userAgent}
          disabled={disabled}
          autoComplete="off"
          maxLength={8192}
          placeholder={t("requestProfile.defaultUserAgent")}
          aria-describedby={hintId}
          className="h-8"
          onChange={(event) => onChange({ ...value, userAgent: event.target.value })}
        />
      </label>
      <label htmlFor={`${idPrefix}-referer`} className="flex flex-col gap-1 text-xs text-text-muted">
        Referer
        <Input
          id={`${idPrefix}-referer`}
          value={value.referer}
          disabled={disabled}
          autoComplete="off"
          maxLength={8192}
          placeholder="https://example.com/downloads"
          aria-describedby={hintId}
          className="h-8"
          onChange={(event) => onChange({ ...value, referer: event.target.value })}
        />
      </label>
      <label htmlFor={`${idPrefix}-custom`} className="flex flex-col gap-1 text-xs text-text-muted">
        {t("requestProfile.customHeaders")}
        <textarea
          id={`${idPrefix}-custom`}
          value={value.customHeaders}
          disabled={disabled}
          autoComplete="off"
          spellCheck={false}
          rows={3}
          maxLength={16384}
          aria-describedby={hintId}
          placeholder={t("requestProfile.customPlaceholder")}
          className="min-h-20 w-full resize-y rounded-md border border-border-subtle bg-surface-root px-2 py-1.5 font-mono text-xs text-text-primary placeholder:text-text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-primary disabled:opacity-50"
          onChange={(event) => onChange({ ...value, customHeaders: event.target.value })}
        />
      </label>
    </div>
  );
}
