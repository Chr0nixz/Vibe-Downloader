import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { BrowserCaptureSettings, BrowserSiteRule, BrowserSiteRuleMode } from "@/generated/bindings";
import { normalizeSiteRule, validateSiteRule } from "@/lib/browser-capture-draft";
import {
  analyzeSiteRuleConflicts,
  type CaptureDiagnosis,
  diagnoseCaptureUrl,
  type SiteRuleConflict,
} from "@/lib/capture-policy";
import { parseSiteRulesImport, serializeSiteRulesExport } from "@/lib/site-rules-io";
import { UNDO_TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";

type CaptureGlobals = Pick<
  BrowserCaptureSettings,
  "autoIntercept" | "minSizeBytes" | "fileExtensions" | "forwardHeadersMode"
>;

interface SiteRulesEditorProps {
  rules: BrowserSiteRule[];
  captureGlobals: CaptureGlobals;
  disabled?: boolean;
  onUpdate: (rules: BrowserSiteRule[]) => void;
}

/**
 * UX-06: edits stay in a local draft until Save; Cancel never calls onUpdate.
 * Deletes update the parent draft immediately and offer Undo via toast.
 * Diagnostics (try URL, conflicts, import/export) operate on the parent draft list.
 */
export function SiteRulesEditor({ rules, captureGlobals, disabled, onUpdate }: SiteRulesEditorProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((state) => state.addToast);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<BrowserSiteRule | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const rulesRef = useRef(rules);
  useEffect(() => {
    rulesRef.current = rules;
  }, [rules]);

  const conflicts = useMemo(() => analyzeSiteRuleConflicts(rules), [rules]);
  const conflictsByRuleId = useMemo(() => {
    const map = new Map<string, SiteRuleConflict>();
    for (const conflict of conflicts) {
      if (!map.has(conflict.ruleId)) map.set(conflict.ruleId, conflict);
    }
    return map;
  }, [conflicts]);

  function beginAdd() {
    const newRule: BrowserSiteRule = {
      id: crypto.randomUUID(),
      hostPattern: "",
      includeSubdomains: true,
      mode: "auto",
      minSizeBytes: null,
      fileExtensions: [],
      forwardHeaders: null,
    };
    setDraft(newRule);
    setIsNew(true);
    setEditingId(newRule.id);
    setValidationError(null);
  }

  function beginEdit(rule: BrowserSiteRule) {
    setDraft({ ...rule, fileExtensions: [...rule.fileExtensions] });
    setIsNew(false);
    setEditingId(rule.id);
    setValidationError(null);
  }

  function cancelEdit() {
    setEditingId(null);
    setDraft(null);
    setIsNew(false);
    setValidationError(null);
  }

  function saveEdit() {
    if (!draft) return;
    const normalized = normalizeSiteRule(draft);
    const errorKey = validateSiteRule(normalized);
    if (errorKey) {
      setValidationError(errorKey);
      setDraft(normalized);
      return;
    }
    if (isNew) {
      onUpdate([...rulesRef.current, normalized]);
    } else {
      onUpdate(rulesRef.current.map((rule) => (rule.id === normalized.id ? normalized : rule)));
    }
    cancelEdit();
  }

  function deleteRule(id: string) {
    const current = rulesRef.current;
    const index = current.findIndex((rule) => rule.id === id);
    const removed = current[index];
    if (!removed) return;
    onUpdate(current.filter((rule) => rule.id !== id));
    if (editingId === id) cancelEdit();
    addToast({
      tone: "info",
      title: t("settings.siteRuleDeleted"),
      description: t("common.undoHint"),
      durationMs: UNDO_TOAST_TIMEOUT_MS,
      action: {
        label: t("common.undo"),
        onClick: () => {
          const without = rulesRef.current.filter((rule) => rule.id !== removed.id);
          const restored = [...without];
          restored.splice(Math.min(index, restored.length), 0, removed);
          onUpdate(restored);
        },
      },
    });
  }

  function moveRule(index: number, direction: "up" | "down") {
    const target = direction === "up" ? index - 1 : index + 1;
    if (target < 0 || target >= rulesRef.current.length) return;
    const reordered = [...rulesRef.current];
    [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
    onUpdate(reordered);
  }

  function exportRules() {
    const blob = new Blob([serializeSiteRulesExport(rulesRef.current)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "vibe-site-rules.json";
    anchor.click();
    URL.revokeObjectURL(url);
  }

  function importRules(file: File) {
    const reader = new FileReader();
    reader.onload = () => {
      const text = typeof reader.result === "string" ? reader.result : "";
      const result = parseSiteRulesImport(text);
      if (!result.ok) {
        addToast({
          tone: "error",
          title: t("settings.siteRulesImportFailed"),
          description: t(result.errorKey, { detail: result.detail ?? "" }),
        });
        return;
      }
      onUpdate(result.rules);
      cancelEdit();
      addToast({
        tone: "success",
        title: t("settings.siteRulesImported"),
        description: t("settings.siteRulesImportedCount", { count: result.rules.length }),
      });
    };
    reader.onerror = () => {
      addToast({
        tone: "error",
        title: t("settings.siteRulesImportFailed"),
        description: t("settings.siteRulesImportInvalidJson"),
      });
    };
    reader.readAsText(file);
  }

  const showNewEditor = isNew && draft && editingId === draft.id;

  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <h4 className="text-sm font-medium text-text-primary">{t("settings.siteRulesSection")}</h4>
          <p className="mt-1 text-xs leading-5 text-text-muted">{t("settings.siteRulesHint")}</p>
          <p className="mt-1 text-xs leading-5 text-text-muted">{t("settings.siteRulesOrderHint")}</p>
        </div>
        <div className="flex flex-wrap gap-1">
          <button
            type="button"
            onClick={exportRules}
            disabled={disabled || rules.length === 0}
            className="rounded-md border border-border-subtle px-3 py-1.5 text-xs text-text-primary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.siteRulesExport")}
          </button>
          <button
            type="button"
            onClick={() => fileInputRef.current?.click()}
            disabled={disabled || editingId !== null}
            className="rounded-md border border-border-subtle px-3 py-1.5 text-xs text-text-primary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.siteRulesImport")}
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file) importRules(file);
            }}
          />
          <button
            type="button"
            onClick={beginAdd}
            disabled={disabled || editingId !== null}
            className="rounded-md border border-border-subtle px-3 py-1.5 text-xs text-text-primary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.addSiteRule")}
          </button>
        </div>
      </div>

      <SiteRuleTryPanel rules={rules} captureGlobals={captureGlobals} disabled={disabled} />

      {conflicts.length > 0 ? (
        <div
          role="status"
          className="rounded-md border border-status-warning/40 bg-surface-hover px-3 py-2 text-xs text-status-warning"
        >
          {t("settings.siteRulesConflictSummary", { count: conflicts.length })}
        </div>
      ) : null}

      {rules.length === 0 && !showNewEditor ? (
        <p className="text-xs text-text-muted">{t("settings.noSiteRules")}</p>
      ) : (
        <ul className="grid gap-2">
          {rules.map((rule, index) => (
            <li key={rule.id} className="grid gap-2 rounded-md border border-border-divider p-3">
              {editingId === rule.id && draft && !isNew ? (
                <RuleEditForm
                  rule={draft}
                  disabled={disabled}
                  validationError={validationError}
                  onChange={(patch) => {
                    setDraft((current) => (current ? { ...current, ...patch } : current));
                    setValidationError(null);
                  }}
                  onCancel={cancelEdit}
                  onSave={saveEdit}
                  onDelete={() => deleteRule(rule.id)}
                />
              ) : (
                <RuleRow
                  rule={rule}
                  conflict={conflictsByRuleId.get(rule.id) ?? null}
                  rules={rules}
                  disabled={disabled || editingId !== null}
                  canMoveUp={index > 0}
                  canMoveDown={index < rules.length - 1}
                  onEdit={() => beginEdit(rule)}
                  onDelete={() => deleteRule(rule.id)}
                  onMoveUp={() => moveRule(index, "up")}
                  onMoveDown={() => moveRule(index, "down")}
                />
              )}
            </li>
          ))}
          {showNewEditor && draft ? (
            <li className="grid gap-2 rounded-md border border-border-divider p-3">
              <RuleEditForm
                rule={draft}
                disabled={disabled}
                validationError={validationError}
                onChange={(patch) => {
                  setDraft((current) => (current ? { ...current, ...patch } : current));
                  setValidationError(null);
                }}
                onCancel={cancelEdit}
                onSave={saveEdit}
                onDelete={cancelEdit}
              />
            </li>
          ) : null}
        </ul>
      )}
    </div>
  );
}

function SiteRuleTryPanel({
  rules,
  captureGlobals,
  disabled,
}: {
  rules: BrowserSiteRule[];
  captureGlobals: CaptureGlobals;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [url, setUrl] = useState("https://cdn.example.com/video.mp4");
  const [filename, setFilename] = useState("video.mp4");
  const [sizeMiB, setSizeMiB] = useState("10");
  const [error, setError] = useState<string | null>(null);
  const [diagnosis, setDiagnosis] = useState<CaptureDiagnosis | null>(null);

  function runDiagnosis() {
    try {
      const totalBytes = Number.parseFloat(sizeMiB);
      const bytes = Number.isFinite(totalBytes) && totalBytes > 0 ? Math.round(totalBytes * 1024 * 1024) : 0;
      const result = diagnoseCaptureUrl(
        url.trim(),
        {
          autoIntercept: captureGlobals.autoIntercept,
          forwardHeadersMode: captureGlobals.forwardHeadersMode,
          minSizeBytes: captureGlobals.minSizeBytes,
          fileExtensions: captureGlobals.fileExtensions,
          siteRules: rules,
        },
        { filename: filename.trim() || null, totalBytes: bytes },
      );
      setDiagnosis(result);
      setError(null);
    } catch {
      setDiagnosis(null);
      setError("settings.siteRulesTryInvalidUrl");
    }
  }

  return (
    <div className="grid gap-2 rounded-md border border-border-divider p-3">
      <div>
        <h5 className="text-xs font-medium text-text-primary">{t("settings.siteRulesTryTitle")}</h5>
        <p className="mt-1 text-xs text-text-muted">{t("settings.siteRulesTryHint")}</p>
      </div>
      <Field label={t("settings.siteRulesTryUrl")}>
        <Input
          aria-label={t("settings.siteRulesTryUrl")}
          type="url"
          value={url}
          disabled={disabled}
          onChange={(e) => setUrl(e.target.value)}
          className="max-w-xl"
        />
      </Field>
      <Field label={t("settings.siteRulesTryFilename")}>
        <Input
          aria-label={t("settings.siteRulesTryFilename")}
          type="text"
          value={filename}
          disabled={disabled}
          onChange={(e) => setFilename(e.target.value)}
          className="max-w-md"
        />
      </Field>
      <Field label={t("settings.siteRulesTrySize")}>
        <Input
          aria-label={t("settings.siteRulesTrySize")}
          type="number"
          min={0}
          step={0.1}
          value={sizeMiB}
          disabled={disabled}
          onChange={(e) => setSizeMiB(e.target.value)}
          className="max-w-xs"
        />
      </Field>
      <div className="flex justify-end">
        <button
          type="button"
          onClick={runDiagnosis}
          disabled={disabled || !url.trim()}
          className="rounded-md bg-accent-primary px-3 py-1.5 text-xs text-text-on-accent hover:opacity-90 disabled:opacity-50"
        >
          {t("settings.siteRulesTryRun")}
        </button>
      </div>
      {error ? (
        <p role="alert" className="text-xs text-status-danger">
          {t(error)}
        </p>
      ) : null}
      {diagnosis ? <DiagnosisResult diagnosis={diagnosis} /> : null}
    </div>
  );
}

function DiagnosisResult({ diagnosis }: { diagnosis: CaptureDiagnosis }) {
  const { t } = useTranslation();
  const matched = diagnosis.matchedRule;
  const interceptLabel = diagnosis.intercept.intercept
    ? t("settings.siteRulesTryInterceptYes")
    : t(interceptReasonKey(diagnosis.intercept.reason));
  const headerLabel = t(headerStateKey(diagnosis.headers.state));
  return (
    <dl className="grid gap-1 text-xs text-text-secondary">
      <div>
        <dt className="inline text-text-muted">{t("settings.siteRulesTryMatched")}: </dt>
        <dd className="inline text-text-primary">
          {matched?.hostPattern ?? t("settings.siteRulesTryNoMatch")}
          {matched ? ` (${t(`settings.ruleMode${capitalize(matched.mode)}`)})` : ""}
        </dd>
      </div>
      <div>
        <dt className="inline text-text-muted">{t("settings.siteRulesTryIntercept")}: </dt>
        <dd className="inline text-text-primary">{interceptLabel}</dd>
      </div>
      <div>
        <dt className="inline text-text-muted">{t("settings.siteRulesTryHeaders")}: </dt>
        <dd className="inline text-text-primary">{headerLabel}</dd>
      </div>
      <div>
        <dt className="inline text-text-muted">{t("settings.siteRulesTryEffectiveMinSize")}: </dt>
        <dd className="inline text-text-primary">
          {diagnosis.effectiveMinSizeBytes > 0
            ? `${(diagnosis.effectiveMinSizeBytes / (1024 * 1024)).toFixed(2)} MiB`
            : t("settings.siteRulesTryNoMinSize")}
        </dd>
      </div>
      <div>
        <dt className="inline text-text-muted">{t("settings.siteRulesTryEffectiveExt")}: </dt>
        <dd className="inline text-text-primary">
          {diagnosis.effectiveFileExtensions.length > 0
            ? diagnosis.effectiveFileExtensions.join(", ")
            : t("settings.siteRulesTryAnyExt")}
        </dd>
      </div>
    </dl>
  );
}

function interceptReasonKey(reason: string | undefined): string {
  switch (reason) {
    case "site-rule":
      return "settings.siteRulesTryReasonSiteRule";
    case "ask-rule":
      return "settings.siteRulesTryReasonAskRule";
    case "size":
      return "settings.siteRulesTryReasonSize";
    case "extension":
      return "settings.siteRulesTryReasonExtension";
    default:
      return "settings.siteRulesTryReasonDisabled";
  }
}

function headerStateKey(state: string): string {
  switch (state) {
    case "allowed":
      return "settings.siteRulesTryHeaderAllowed";
    case "ask":
      return "settings.siteRulesTryHeaderAsk";
    default:
      return "settings.siteRulesTryHeaderDenied";
  }
}

function RuleRow({
  rule,
  conflict,
  rules,
  disabled,
  canMoveUp,
  canMoveDown,
  onEdit,
  onDelete,
  onMoveUp,
  onMoveDown,
}: {
  rule: BrowserSiteRule;
  conflict: SiteRuleConflict | null;
  rules: BrowserSiteRule[];
  disabled?: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const { t } = useTranslation();
  const byHost = conflict ? rules.find((item) => item.id === conflict.byRuleId)?.hostPattern : null;
  return (
    <div className="grid gap-2">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm text-text-primary">
            {rule.hostPattern || t("settings.ruleHostPatternPlaceholder")}
          </div>
          <div className="text-xs text-text-muted">
            {t(`settings.ruleMode${capitalize(rule.mode)}`)}
            {rule.includeSubdomains ? ` · ${t("settings.ruleIncludeSubdomains")}` : ""}
          </div>
        </div>
        <div className="flex gap-1">
          <button
            type="button"
            onClick={onMoveUp}
            disabled={disabled || !canMoveUp}
            className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.moveUp")}
          </button>
          <button
            type="button"
            onClick={onMoveDown}
            disabled={disabled || !canMoveDown}
            className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.moveDown")}
          </button>
          <button
            type="button"
            onClick={onEdit}
            disabled={disabled}
            className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.editRule")}
          </button>
          <button
            type="button"
            onClick={onDelete}
            disabled={disabled}
            className="rounded px-2 py-1 text-xs text-status-danger hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.deleteRule")}
          </button>
        </div>
      </div>
      {conflict ? (
        <p className="text-xs text-status-warning">
          {conflict.kind === "shadowed"
            ? t("settings.siteRulesConflictShadowed", { host: byHost ?? conflict.byRuleId })
            : t("settings.siteRulesConflictOverlap", { host: byHost ?? conflict.byRuleId })}
        </p>
      ) : null}
    </div>
  );
}

function RuleEditForm({
  rule,
  disabled,
  validationError,
  onChange,
  onCancel,
  onSave,
  onDelete,
}: {
  rule: BrowserSiteRule;
  disabled?: boolean;
  validationError: string | null;
  onChange: (patch: Partial<BrowserSiteRule>) => void;
  onCancel: () => void;
  onSave: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-3">
      <Field label={t("settings.ruleHostPattern")}>
        <Input
          aria-label={t("settings.ruleHostPattern")}
          type="text"
          value={rule.hostPattern}
          disabled={disabled}
          onChange={(e) => onChange({ hostPattern: e.target.value })}
          placeholder="*.example.com"
        />
      </Field>
      <Field label={t("settings.ruleIncludeSubdomains")}>
        <Switch
          aria-label={t("settings.ruleIncludeSubdomains")}
          checked={rule.includeSubdomains}
          disabled={disabled}
          onCheckedChange={(checked) => onChange({ includeSubdomains: checked })}
        />
      </Field>
      <Field label={t("settings.ruleMode")}>
        <Select
          value={rule.mode}
          onValueChange={(value) => onChange({ mode: value as BrowserSiteRuleMode })}
          disabled={disabled}
        >
          <SelectTrigger aria-label={t("settings.ruleMode")} className="w-full max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="auto">{t("settings.ruleModeAuto")}</SelectItem>
            <SelectItem value="ask">{t("settings.ruleModeAsk")}</SelectItem>
            <SelectItem value="never">{t("settings.ruleModeNever")}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      <Field label={t("settings.ruleMinSize")}>
        <Input
          aria-label={t("settings.ruleMinSize")}
          type="number"
          min={0}
          step={1}
          value={rule.minSizeBytes ? Number.parseInt(rule.minSizeBytes, 10) / (1024 * 1024) : ""}
          disabled={disabled}
          onChange={(e) => {
            const mib = Number.parseFloat(e.target.value);
            onChange({
              minSizeBytes: Number.isFinite(mib) && mib > 0 ? String(Math.round(mib * 1024 * 1024)) : null,
            });
          }}
          placeholder="0"
          className="max-w-xs"
        />
      </Field>
      <Field label={t("settings.ruleFileExtensions")}>
        <Input
          aria-label={t("settings.ruleFileExtensions")}
          type="text"
          value={rule.fileExtensions.join(", ")}
          disabled={disabled}
          onChange={(e) =>
            onChange({
              fileExtensions: e.target.value
                .split(",")
                .map((ext) => ext.trim())
                .filter((ext) => ext.length > 0),
            })
          }
          placeholder="mp4, mkv"
          className="max-w-md"
        />
      </Field>
      <Field label={t("settings.ruleForwardHeaders")}>
        <Select
          value={rule.forwardHeaders === null ? "default" : rule.forwardHeaders ? "true" : "false"}
          onValueChange={(value) => {
            const forwardHeaders = value === "default" ? null : value === "true";
            onChange({ forwardHeaders });
          }}
          disabled={disabled}
        >
          <SelectTrigger aria-label={t("settings.ruleForwardHeaders")} className="w-full max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="default">{t("settings.ruleForwardHeadersDefault")}</SelectItem>
            <SelectItem value="true">{t("settings.ruleForwardHeadersTrue")}</SelectItem>
            <SelectItem value="false">{t("settings.ruleForwardHeadersFalse")}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      {validationError ? (
        <p role="alert" className="text-xs text-status-danger">
          {t(validationError)}
        </p>
      ) : null}
      <div className="flex justify-between gap-2">
        <button
          type="button"
          onClick={onDelete}
          disabled={disabled}
          className="rounded px-3 py-1.5 text-xs text-status-danger hover:bg-surface-hover disabled:opacity-50"
        >
          {t("settings.deleteRule")}
        </button>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={disabled}
            className="rounded-md border border-border-subtle px-3 py-1.5 text-xs text-text-secondary hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.cancelRule")}
          </button>
          <button
            type="button"
            onClick={onSave}
            disabled={disabled}
            className="rounded-md bg-accent-primary px-3 py-1.5 text-xs text-text-on-accent hover:opacity-90 disabled:opacity-50"
          >
            {t("settings.saveRule")}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1 sm:grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] sm:items-center">
      <span className="text-xs text-text-secondary">{label}</span>
      <div>{children}</div>
    </div>
  );
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
