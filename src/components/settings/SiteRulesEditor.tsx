import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { BrowserSiteRule, BrowserSiteRuleMode } from "@/generated/bindings";
import { normalizeSiteRule, validateSiteRule } from "@/lib/browser-capture-draft";
import { UNDO_TOAST_TIMEOUT_MS, useToastStore } from "@/stores/toast-store";

interface SiteRulesEditorProps {
  rules: BrowserSiteRule[];
  disabled?: boolean;
  onUpdate: (rules: BrowserSiteRule[]) => void;
}

/**
 * UX-06: edits stay in a local draft until Save; Cancel never calls onUpdate.
 * Deletes update the parent draft immediately and offer Undo via toast.
 */
export function SiteRulesEditor({ rules, disabled, onUpdate }: SiteRulesEditorProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((state) => state.addToast);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<BrowserSiteRule | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);
  const rulesRef = useRef(rules);
  useEffect(() => {
    rulesRef.current = rules;
  }, [rules]);

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
          // Rebuild from the latest parent list without the removed id, then
          // re-insert. Do not early-return when the parent has not yet applied
          // the delete (rulesRef may still contain the removed rule).
          const without = rulesRef.current.filter((rule) => rule.id !== removed.id);
          const restored = [...without];
          restored.splice(Math.min(index, restored.length), 0, removed);
          onUpdate(restored);
        },
      },
    });
  }

  const showNewEditor = isNew && draft && editingId === draft.id;

  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4">
      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-medium text-text-primary">{t("settings.siteRulesSection")}</h4>
          <p className="mt-1 text-xs leading-5 text-text-muted">{t("settings.siteRulesHint")}</p>
        </div>
        <button
          type="button"
          onClick={beginAdd}
          disabled={disabled || editingId !== null}
          className="rounded-md border border-border-subtle px-3 py-1.5 text-xs text-text-primary hover:bg-surface-hover disabled:opacity-50"
        >
          {t("settings.addSiteRule")}
        </button>
      </div>
      {rules.length === 0 && !showNewEditor ? (
        <p className="text-xs text-text-muted">{t("settings.noSiteRules")}</p>
      ) : (
        <ul className="grid gap-2">
          {rules.map((rule) => (
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
                  disabled={disabled || editingId !== null}
                  onEdit={() => beginEdit(rule)}
                  onDelete={() => deleteRule(rule.id)}
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

function RuleRow({
  rule,
  disabled,
  onEdit,
  onDelete,
}: {
  rule: BrowserSiteRule;
  disabled?: boolean;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="min-w-0">
        <div className="text-sm text-text-primary">{rule.hostPattern || t("settings.ruleHostPatternPlaceholder")}</div>
        <div className="text-xs text-text-muted">
          {t(`settings.ruleMode${capitalize(rule.mode)}`)}
          {rule.includeSubdomains ? ` · ${t("settings.ruleIncludeSubdomains")}` : ""}
        </div>
      </div>
      <div className="flex gap-1">
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
