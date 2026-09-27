//! TaskTimeline: milestone history in the Overview tab (feature proposal §4.3).
//!
//! Shows only milestone events from the task event log; engine chatter stays
//! in the Logs tab. Trigger badges come from `TIMELINE_EVENT_TRIGGERS`, which
//! maps the event vocabulary to a source — ambiguous events get no badge.

import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { TaskEvent } from "@/generated/bindings";
import { formatDateTime } from "@/lib/format-date";
import {
  TIMELINE_TRIGGER_KEYS,
  type TimelineTrigger,
  timelineMilestones,
  timelinePayloadSummary,
} from "@/lib/integrity-passport";
import { cn } from "@/lib/utils";

const RECENT_MILESTONES = 8;

const TRIGGER_DOT_CLASS: Record<TimelineTrigger, string> = {
  user: "bg-accent-primary",
  scheduler: "bg-text-muted/60",
  remote: "bg-status-warning/80",
  error: "bg-status-danger",
  engine: "bg-text-muted/60",
  verification: "bg-status-success",
};

export function TaskTimeline({
  events,
  error,
  onOpenLogs,
}: {
  events: TaskEvent[];
  error: string | null;
  onOpenLogs: () => void;
}) {
  const { t } = useTranslation();
  const milestones = useMemo(() => timelineMilestones(events).slice(0, RECENT_MILESTONES), [events]);

  return (
    <div className="rounded-md border border-border-subtle bg-surface-raised/40 p-3 text-xs">
      <div className="flex items-center justify-between gap-3">
        <span className="text-text-muted">{t("taskDetails.timeline.title")}</span>
        <Button type="button" size="sm" variant="ghost" className="h-7 shrink-0 px-2 text-[11px]" onClick={onOpenLogs}>
          {t("taskDetails.timeline.viewAll")}
        </Button>
      </div>
      {error ? (
        <p role="alert" className="mt-1 text-[11px] text-status-danger">
          {error}
        </p>
      ) : null}
      {!error && milestones.length === 0 ? (
        <p className="mt-2 text-[11px] text-text-muted">{t("taskDetails.timeline.empty")}</p>
      ) : null}
      {milestones.length > 0 ? (
        <ol className="mt-2 space-y-1.5">
          {milestones.map((milestone) => {
            const summary = timelinePayloadSummary(milestone.payload, t);
            return (
              <li key={milestone.id} className="flex items-start gap-2">
                <span
                  aria-hidden
                  className={cn(
                    "mt-1.5 size-1.5 shrink-0 rounded-full",
                    milestone.trigger ? TRIGGER_DOT_CLASS[milestone.trigger] : "bg-text-muted/40",
                  )}
                />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5">
                    <span className="font-medium text-text-primary">
                      {milestone.labelKey ? t(milestone.labelKey) : milestone.eventType}
                    </span>
                    {milestone.trigger ? (
                      <span className="rounded bg-surface-raised px-1.5 py-0.5 text-[11px] text-text-muted">
                        {t(TIMELINE_TRIGGER_KEYS[milestone.trigger])}
                      </span>
                    ) : null}
                    <span className="ml-auto font-mono text-[11px] tabular-nums text-text-muted">
                      {formatDateTime(milestone.createdAt, "dateTime")}
                    </span>
                  </div>
                  {summary ? (
                    <p className="truncate text-[11px] text-text-muted" title={summary}>
                      {summary}
                    </p>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ol>
      ) : null}
    </div>
  );
}
