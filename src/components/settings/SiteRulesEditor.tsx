import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { BrowserSiteRule, BrowserSiteRuleMode } from "@/generated/bindings";

interface SiteRulesEditorProps {
  rules: BrowserSiteRule[];
  disabled?: boolean;
  onUpdate: (rules: BrowserSiteRule[]) => void;
}

export function SiteRulesEditor({ rules, disabled, onUpdate }: SiteRulesEditorProps) {
  const { t } = useTranslation();
  const [editingId, setEditingId] = useState<string | null>(null);

  function addRule() {
    const newRule: BrowserSiteRule = {
      id: crypto.randomUUID(),
      hostPattern: "",
      includeSubdomains: true,
      mode: "auto",
      minSizeBytes: null,
      fileExtensions: [],
      forwardHeaders: null,
    };
    onUpdate([...rules, newRule]);
    setEditingId(newRule.id);
  }

  function updateRule(id: string, patch: Partial<BrowserSiteRule>) {
    onUpdate(rules.map((rule) => (rule.id === id ? { ...rule, ...patch } : rule)));
  }

  function deleteRule(id: string) {
    onUpdate(rules.filter((rule) => rule.id !== id));
    if (editingId === id) setEditingId(null);
  }

  return (
    <div className="grid gap-3 border-t border-border-divider px-4 py-4">
      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-medium text-text-primary">{t("settings.siteRulesSection")}</h4>
          <p className="mt-1 text-xs leading-5 text-text-muted">{t("settings.siteRulesHint")}</p>
        </div>
        <button
          type="button"
          onClick={addRule}
          disabled={disabled}
          className="rounded-md border border-border-input px-3 py-1.5 text-xs text-text-primary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.addSiteRule")}
        </button>
      </div>
      {rules.length === 0 ? (
        <p className="text-xs text-text-muted">{t("settings.noSiteRules")}</p>
      ) : (
        <ul className="grid gap-2">
          {rules.map((rule) => (
            <li key={rule.id} className="grid gap-2 rounded-md border border-border-divider p-3">
              {editingId === rule.id ? (
                <RuleEditForm
                  rule={rule}
                  disabled={disabled}
                  onChange={(patch) => updateRule(rule.id, patch)}
                  onDone={() => setEditingId(null)}
                  onDelete={() => deleteRule(rule.id)}
                />
              ) : (
                <RuleRow
                  rule={rule}
                  disabled={disabled}
                  onEdit={() => setEditingId(rule.id)}
                  onDelete={() => deleteRule(rule.id)}
                />
              )}
            </li>
          ))}
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
          className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.editRule")}
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={disabled}
          className="rounded px-2 py-1 text-xs text-text-danger hover:bg-bg-hover disabled:opacity-50"
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
  onChange,
  onDone,
  onDelete,
}: {
  rule: BrowserSiteRule;
  disabled?: boolean;
  onChange: (patch: Partial<BrowserSiteRule>) => void;
  onDone: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid gap-3">
      <Field label={t("settings.ruleHostPattern")}>
        <input
          type="text"
          value={rule.hostPattern}
          disabled={disabled}
          onChange={(e) => onChange({ hostPattern: e.target.value })}
          placeholder="*.example.com"
          className="w-full rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
        />
      </Field>
      <Field label={t("settings.ruleIncludeSubdomains")}>
        <input
          type="checkbox"
          checked={rule.includeSubdomains}
          disabled={disabled}
          onChange={(e) => onChange({ includeSubdomains: e.target.checked })}
          className="h-5 w-5 accent-accent-primary"
        />
      </Field>
      <Field label={t("settings.ruleMode")}>
        <Select
          value={rule.mode}
          onValueChange={(value) => onChange({ mode: value as BrowserSiteRuleMode })}
          disabled={disabled}
        >
          <SelectTrigger className="w-full max-w-xs">
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
        <input
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
          className="w-full max-w-xs rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
        />
      </Field>
      <Field label={t("settings.ruleFileExtensions")}>
        <input
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
          className="w-full max-w-md rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
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
          <SelectTrigger className="w-full max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="default">{t("settings.ruleForwardHeadersDefault")}</SelectItem>
            <SelectItem value="true">{t("settings.ruleForwardHeadersTrue")}</SelectItem>
            <SelectItem value="false">{t("settings.ruleForwardHeadersFalse")}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      <div className="flex justify-between">
        <button
          type="button"
          onClick={onDelete}
          disabled={disabled}
          className="rounded px-3 py-1.5 text-xs text-text-danger hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.deleteRule")}
        </button>
        <button
          type="button"
          onClick={onDone}
          disabled={disabled}
          className="rounded-md bg-accent-primary px-3 py-1.5 text-xs text-text-on-accent hover:opacity-90 disabled:opacity-50"
        >
          {t("settings.saveRule")}
        </button>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1 sm:grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] sm:items-center">
      <label className="text-xs text-text-secondary">{label}</label>
      <div>{children}</div>
    </div>
  );
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
