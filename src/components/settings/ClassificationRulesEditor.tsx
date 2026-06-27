import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { ClassificationMatchKind, ClassificationRule, ClassificationRuleInput } from "@/generated/bindings";
import { localizedErrorMessage } from "@/lib/errors";
import {
  createClassificationRule,
  deleteClassificationRule,
  listClassificationRules,
  reorderClassificationRules,
  updateClassificationRule,
} from "@/lib/tauri";
import { useToastStore } from "@/stores/toast-store";

export function ClassificationRulesEditor() {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [rules, setRules] = useState<ClassificationRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const result = await listClassificationRules();
      setRules(result);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setLoading(false);
    }
  }, [addToast, t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleCreate(input: ClassificationRuleInput) {
    setSaving(true);
    try {
      await createClassificationRule(input);
      await refresh();
    } catch (err) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setSaving(false);
    }
  }

  async function handleUpdate(id: string, input: ClassificationRuleInput) {
    setSaving(true);
    try {
      await updateClassificationRule(id, input);
      await refresh();
    } catch (err) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: string) {
    setSaving(true);
    try {
      await deleteClassificationRule(id);
      await refresh();
    } catch (err) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
    } finally {
      setSaving(false);
    }
  }

  async function handleMove(index: number, direction: "up" | "down") {
    const target = direction === "up" ? index - 1 : index + 1;
    if (target < 0 || target >= rules.length) return;
    const reordered = [...rules];
    [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
    const ids = reordered.map((rule) => rule.id);
    setRules(reordered);
    try {
      await reorderClassificationRules(ids);
    } catch (err) {
      addToast({
        tone: "error",
        title: t("toast.actionFailed"),
        description: localizedErrorMessage(err, t),
      });
      await refresh();
    }
  }

  return (
    <div className="grid gap-3 px-4 py-4">
      <div className="flex items-center justify-between">
        <div>
          <h4 className="text-sm font-medium text-text-primary">{t("settings.classificationSection")}</h4>
          <p className="mt-1 text-xs leading-5 text-text-muted">{t("settings.classificationHint")}</p>
        </div>
        {editingId !== "new" ? (
          <button
            type="button"
            onClick={() => setEditingId("new")}
            disabled={saving || loading}
            className="rounded-md border border-border-input px-3 py-1.5 text-xs text-text-primary hover:bg-bg-hover disabled:opacity-50"
          >
            {t("settings.addClassificationRule")}
          </button>
        ) : null}
      </div>
      {loading ? (
        <p className="text-xs text-text-muted">{t("settings.classificationLoading")}</p>
      ) : rules.length === 0 && editingId !== "new" ? (
        <p className="text-xs text-text-muted">{t("settings.noClassificationRules")}</p>
      ) : (
        <ul className="grid gap-2">
          {rules.map((rule, index) => (
            <li key={rule.id} className="grid gap-2 rounded-md border border-border-divider p-3">
              {editingId === rule.id ? (
                <RuleEditForm
                  initial={rule}
                  disabled={saving}
                  onSubmit={(input) => {
                    void handleUpdate(rule.id, input);
                    setEditingId(null);
                  }}
                  onCancel={() => setEditingId(null)}
                />
              ) : (
                <RuleRow
                  rule={rule}
                  disabled={saving}
                  canMoveUp={index > 0}
                  canMoveDown={index < rules.length - 1}
                  onEdit={() => setEditingId(rule.id)}
                  onDelete={() => void handleDelete(rule.id)}
                  onMoveUp={() => void handleMove(index, "up")}
                  onMoveDown={() => void handleMove(index, "down")}
                />
              )}
            </li>
          ))}
          {editingId === "new" ? (
            <li className="grid gap-2 rounded-md border border-border-divider p-3">
              <RuleEditForm
                initial={null}
                disabled={saving}
                onSubmit={(input) => {
                  void handleCreate(input);
                  setEditingId(null);
                }}
                onCancel={() => setEditingId(null)}
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
  canMoveUp,
  canMoveDown,
  onEdit,
  onDelete,
  onMoveUp,
  onMoveDown,
}: {
  rule: ClassificationRule;
  disabled?: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-text-primary">{rule.name}</span>
          {!rule.enabled ? (
            <span className="rounded bg-bg-muted px-1.5 py-0.5 text-xs text-text-muted">
              {t("settings.ruleDisabled")}
            </span>
          ) : null}
        </div>
        <div className="text-xs text-text-muted">
          {t(`settings.matchKind${capitalize(rule.matchKind)}`)}: {rule.pattern} → {rule.targetSubdir}
        </div>
      </div>
      <div className="flex gap-1">
        <button
          type="button"
          onClick={onMoveUp}
          disabled={disabled || !canMoveUp}
          className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.moveUp")}
        </button>
        <button
          type="button"
          onClick={onMoveDown}
          disabled={disabled || !canMoveDown}
          className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.moveDown")}
        </button>
        <button
          type="button"
          onClick={onEdit}
          disabled={disabled}
          className="rounded px-2 py-1 text-xs text-text-secondary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.editClassificationRule")}
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={disabled}
          className="rounded px-2 py-1 text-xs text-text-danger hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.deleteClassificationRule")}
        </button>
      </div>
    </div>
  );
}

function RuleEditForm({
  initial,
  disabled,
  onSubmit,
  onCancel,
}: {
  initial: ClassificationRule | null;
  disabled?: boolean;
  onSubmit: (input: ClassificationRuleInput) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(initial?.name ?? "");
  const [matchKind, setMatchKind] = useState<ClassificationMatchKind>(initial?.matchKind ?? "extension");
  const [pattern, setPattern] = useState(initial?.pattern ?? "");
  const [targetSubdir, setTargetSubdir] = useState(initial?.targetSubdir ?? "");
  const [enabled, setEnabled] = useState(initial?.enabled ?? true);

  function handleSubmit() {
    onSubmit({
      name: name.trim() || null,
      matchKind,
      pattern: pattern.trim() || null,
      targetSubdir: targetSubdir.trim() || null,
      enabled,
      position: null,
    });
  }

  return (
    <div className="grid gap-3">
      <Field label={t("settings.ruleName")}>
        <input
          type="text"
          value={name}
          disabled={disabled}
          onChange={(e) => setName(e.target.value)}
          className="w-full rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
        />
      </Field>
      <Field label={t("settings.matchKind")}>
        <Select
          value={matchKind}
          onValueChange={(value) => setMatchKind(value as ClassificationMatchKind)}
          disabled={disabled}
        >
          <SelectTrigger className="w-full max-w-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="extension">{t("settings.matchKindExtension")}</SelectItem>
            <SelectItem value="mime">{t("settings.matchKindMime")}</SelectItem>
            <SelectItem value="url_contains">{t("settings.matchKindUrlContains")}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      <Field label={t("settings.pattern")}>
        <input
          type="text"
          value={pattern}
          disabled={disabled}
          onChange={(e) => setPattern(e.target.value)}
          placeholder={t(`settings.patternHint${capitalize(matchKind)}`)}
          className="w-full max-w-md rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
        />
      </Field>
      <Field label={t("settings.targetSubdir")}>
        <input
          type="text"
          value={targetSubdir}
          disabled={disabled}
          onChange={(e) => setTargetSubdir(e.target.value)}
          placeholder="videos"
          className="w-full max-w-md rounded-md border border-border-input bg-bg-input px-3 py-2 text-sm text-text-primary"
        />
      </Field>
      <Field label={t("settings.ruleEnabled")}>
        <input
          type="checkbox"
          checked={enabled}
          disabled={disabled}
          onChange={(e) => setEnabled(e.target.checked)}
          className="h-5 w-5 accent-accent-primary"
        />
      </Field>
      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          disabled={disabled}
          className="rounded-md border border-border-input px-3 py-1.5 text-xs text-text-secondary hover:bg-bg-hover disabled:opacity-50"
        >
          {t("settings.ruleCancel")}
        </button>
        <button
          type="button"
          onClick={handleSubmit}
          disabled={disabled || !name.trim() || !pattern.trim() || !targetSubdir.trim()}
          className="rounded-md bg-accent-primary px-3 py-1.5 text-xs text-text-on-accent hover:opacity-90 disabled:opacity-50"
        >
          {t("settings.saveClassificationRule")}
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
  if (value === "url_contains") return "UrlContains";
  return value.charAt(0).toUpperCase() + value.slice(1);
}
