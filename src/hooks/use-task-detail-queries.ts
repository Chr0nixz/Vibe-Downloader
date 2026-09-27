import { useCallback, useEffect, useState } from "react";
import type {
  DashSegmentView,
  HlsSegmentView,
  IntegrityPassport,
  RequestDiagnostic,
  SegmentSummary,
  SftpKnownHost,
  TaskEvent,
  TorrentRuntimeSnapshot,
} from "@/generated/bindings";
import { useVisibilityGatedPoll } from "@/hooks/use-visibility-gated-poll";
import { hasByteRangeSegments } from "@/lib/chunk-map";
import { errorMessage } from "@/lib/errors";
import { isDashProtocol, isFtpSftpProtocol, isHlsProtocol, isTorrentProtocol } from "@/lib/task-diagnostics";
import {
  getIntegrityPassport,
  getSegmentSummary,
  getTorrentRuntimeSnapshot,
  listDashSegmentsPage,
  listHlsSegmentsPage,
  listSegmentsPage,
  listSftpKnownHosts,
  listTaskEventsPage,
  listTaskRequestsPage,
} from "@/lib/tauri";
import type { Task } from "@/types/task";
import type { TaskSegment } from "@/types/task-segment";

const SEGMENT_REFRESH_MS = 2_000;
const DETAIL_REFRESH_MS = 30_000;

function mergeById<T extends { id: string }>(current: T[], incoming: T[]): T[] {
  const byId = new Map(current.map((item) => [item.id, item] as const));
  const order = current.map((item) => item.id);
  for (const item of incoming) {
    if (!byId.has(item.id)) order.push(item.id);
    byId.set(item.id, item);
  }
  return order.map((id) => byId.get(id)).filter((item): item is T => Boolean(item));
}

function isLiveStatus(status: Task["status"]): boolean {
  return status === "downloading" || status === "retrying" || status === "queued";
}

function usesPlaylistSegments(protocol: string): boolean {
  return isHlsProtocol(protocol) || isDashProtocol(protocol);
}

/**
 * ARC-17: TaskDetails query controller — segments / requests / events / torrent / HLS / DASH
 * polling gated by the visible tab so inactive panes do not keep hitting IPC.
 */
export function useTaskDetailQueries(options: { task: Task; activeTab: string }) {
  const { task, activeTab } = options;
  const isTorrentTask = isTorrentProtocol(task.protocol);
  const isHlsTask = isHlsProtocol(task.protocol);
  const isDashTask = isDashProtocol(task.protocol);
  const isFtpSftpTask = isFtpSftpProtocol(task.protocol);
  const overviewVisible = activeTab === "overview";
  // Diagnostics is one scroll (segments, then requests), so both load while it
  // is open. The overview's chunk map draws the same work-unit segments, so
  // they load there too — but only for protocols whose segments are byte
  // ranges of one file (a few rows: HTTP caps at 8 ranges, FTP at 4).
  const diagnosticsVisible = activeTab === "diagnostics";
  const segmentsVisible =
    !isTorrentTask && (diagnosticsVisible || (overviewVisible && hasByteRangeSegments(task.protocol)));

  const [segments, setSegments] = useState<TaskSegment[]>([]);
  const [segmentsCursor, setSegmentsCursor] = useState<string | null>(null);
  const [segmentError, setSegmentError] = useState<string | null>(null);
  const [hlsSegments, setHlsSegments] = useState<HlsSegmentView[]>([]);
  const [hlsSegmentsCursor, setHlsSegmentsCursor] = useState<string | null>(null);
  const [hlsSegmentError, setHlsSegmentError] = useState<string | null>(null);
  const [dashSegments, setDashSegments] = useState<DashSegmentView[]>([]);
  const [dashSegmentsCursor, setDashSegmentsCursor] = useState<string | null>(null);
  const [dashSegmentError, setDashSegmentError] = useState<string | null>(null);
  const [events, setEvents] = useState<TaskEvent[]>([]);
  const [eventsCursor, setEventsCursor] = useState<string | null>(null);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [requests, setRequests] = useState<RequestDiagnostic[]>([]);
  const [requestsCursor, setRequestsCursor] = useState<string | null>(null);
  const [requestsError, setRequestsError] = useState<string | null>(null);
  const [torrentSnapshot, setTorrentSnapshot] = useState<TorrentRuntimeSnapshot | null>(null);
  const [torrentSnapshotError, setTorrentSnapshotError] = useState<string | null>(null);
  const [segmentSummary, setSegmentSummary] = useState<SegmentSummary | null>(null);
  const [segmentSummaryError, setSegmentSummaryError] = useState<string | null>(null);
  const [ftpSftpEvents, setFtpSftpEvents] = useState<TaskEvent[]>([]);
  const [sftpKnownHosts, setSftpKnownHosts] = useState<SftpKnownHost[]>([]);
  const [passport, setPassport] = useState<IntegrityPassport | null>(null);
  const [passportError, setPassportError] = useState<string | null>(null);

  // Reset query panes when the selected task changes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: task.id identity is the intentional reset trigger.
  useEffect(() => {
    setSegments([]);
    setSegmentsCursor(null);
    setSegmentError(null);
    setHlsSegments([]);
    setHlsSegmentsCursor(null);
    setHlsSegmentError(null);
    setDashSegments([]);
    setDashSegmentsCursor(null);
    setDashSegmentError(null);
    setEvents([]);
    setEventsCursor(null);
    setEventsError(null);
    setRequests([]);
    setRequestsCursor(null);
    setRequestsError(null);
    setTorrentSnapshot(null);
    setTorrentSnapshotError(null);
    setSegmentSummary(null);
    setSegmentSummaryError(null);
    setFtpSftpEvents([]);
    setSftpKnownHosts([]);
    setPassport(null);
    setPassportError(null);
  }, [task.id]);

  // PERF-02/PERF-14: HTTP/FTP/etc work-unit segments — not used for HLS/DASH (dedicated panes) or BT (hidden).
  const segmentsEnabled = segmentsVisible && !usesPlaylistSegments(task.protocol);

  useEffect(() => {
    if (segmentsEnabled) return;
    setSegments([]);
    setSegmentError(null);
  }, [segmentsEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      listSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (isStale()) return;
          setSegments((prev) => {
            if (
              prev.length === result.items.length &&
              prev.every(
                (s, i) =>
                  s.id === result.items[i].id &&
                  s.status === result.items[i].status &&
                  s.downloadedUntil === result.items[i].downloadedUntil,
              )
            ) {
              return prev;
            }
            return result.items;
          });
          setSegmentsCursor(result.nextCursor);
          setSegmentError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setSegmentError(errorMessage(error));
        }),
    SEGMENT_REFRESH_MS,
    {
      enabled: segmentsEnabled,
      poll: task.status === "downloading" || task.status === "retrying",
      reloadKey: `${task.id}:${task.protocol}`,
    },
  );

  // HLS real playlist segments — only while Diagnostics is visible.
  const hlsSegmentsEnabled = segmentsVisible && isHlsTask;

  useEffect(() => {
    if (hlsSegmentsEnabled) return;
    setHlsSegments([]);
    setHlsSegmentError(null);
  }, [hlsSegmentsEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      listHlsSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (isStale()) return;
          setHlsSegments(result.items);
          setHlsSegmentsCursor(result.nextCursor);
          setHlsSegmentError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setHlsSegmentError(errorMessage(error));
        }),
    SEGMENT_REFRESH_MS,
    {
      enabled: hlsSegmentsEnabled,
      poll: task.status === "downloading" || task.status === "retrying",
      reloadKey: task.id,
    },
  );

  // DASH real MPD segments — only while Diagnostics is visible.
  const dashSegmentsEnabled = segmentsVisible && isDashTask;

  useEffect(() => {
    if (dashSegmentsEnabled) return;
    setDashSegments([]);
    setDashSegmentError(null);
  }, [dashSegmentsEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      listDashSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (isStale()) return;
          setDashSegments(result.items);
          setDashSegmentsCursor(result.nextCursor);
          setDashSegmentError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setDashSegmentError(errorMessage(error));
        }),
    SEGMENT_REFRESH_MS,
    {
      enabled: dashSegmentsEnabled,
      poll: task.status === "downloading" || task.status === "retrying",
      reloadKey: task.id,
    },
  );

  const requestsEnabled = diagnosticsVisible;

  useEffect(() => {
    if (requestsEnabled) return;
    setRequests([]);
    setRequestsError(null);
  }, [requestsEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      listTaskRequestsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (isStale()) return;
          setRequests(result.items);
          setRequestsCursor(result.nextCursor);
          setRequestsError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setRequestsError(errorMessage(error));
        }),
    DETAIL_REFRESH_MS,
    { enabled: requestsEnabled, poll: isLiveStatus(task.status), reloadKey: task.id },
  );

  const torrentSnapshotEnabled = isTorrentTask && activeTab === "overview";

  useEffect(() => {
    if (torrentSnapshotEnabled) return;
    setTorrentSnapshot(null);
    setTorrentSnapshotError(null);
  }, [torrentSnapshotEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      getTorrentRuntimeSnapshot(task.id)
        .then((snapshot) => {
          if (isStale()) return;
          setTorrentSnapshot(snapshot);
          setTorrentSnapshotError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setTorrentSnapshotError(errorMessage(error));
        }),
    DETAIL_REFRESH_MS,
    { enabled: torrentSnapshotEnabled, poll: isLiveStatus(task.status), reloadKey: task.id },
  );

  // FTP/SFTP Overview: segment summary + recent events (acceleration) + SFTP known hosts.
  const overviewExtrasEnabled = overviewVisible && isFtpSftpTask;

  useEffect(() => {
    if (overviewExtrasEnabled) return;
    setSegmentSummary(null);
    setSegmentSummaryError(null);
    setFtpSftpEvents([]);
    setSftpKnownHosts([]);
  }, [overviewExtrasEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      Promise.all([
        getSegmentSummary(task.id)
          .then((summary) => {
            if (isStale()) return;
            setSegmentSummary(summary);
            setSegmentSummaryError(null);
          })
          .catch((error) => {
            if (isStale()) return;
            setSegmentSummaryError(errorMessage(error));
          }),
        listTaskEventsPage({ taskId: task.id, cursor: null, pageSize: 50 })
          .then((result) => {
            if (isStale()) return;
            setFtpSftpEvents(result.items);
          })
          .catch(() => {
            if (isStale()) return;
            setFtpSftpEvents([]);
          }),
        task.protocol === "sftp"
          ? listSftpKnownHosts()
              .then((hosts) => {
                if (isStale()) return;
                setSftpKnownHosts(hosts);
              })
              .catch(() => {
                if (isStale()) return;
                setSftpKnownHosts([]);
              })
          : Promise.resolve().then(() => {
              if (isStale()) return;
              setSftpKnownHosts([]);
            }),
      ]),
    DETAIL_REFRESH_MS,
    {
      enabled: overviewExtrasEnabled,
      poll: isLiveStatus(task.status),
      reloadKey: `${task.id}:${task.protocol}`,
    },
  );

  const eventsEnabled = activeTab === "logs" || activeTab === "overview";

  useEffect(() => {
    if (eventsEnabled) return;
    setEvents([]);
    setEventsError(null);
  }, [eventsEnabled]);

  useVisibilityGatedPoll(
    (isStale) =>
      listTaskEventsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (isStale()) return;
          setEvents(result.items);
          setEventsCursor(result.nextCursor);
          setEventsError(null);
        })
        .catch((error) => {
          if (isStale()) return;
          setEventsError(errorMessage(error));
        }),
    DETAIL_REFRESH_MS,
    { enabled: eventsEnabled, poll: isLiveStatus(task.status), reloadKey: task.id },
  );

  // Integrity passport: fetched while Overview is visible; no polling, but
  // the status dependency refetches when a task transitions to completed.
  // biome-ignore lint/correctness/useExhaustiveDependencies: task.status is the intentional refetch trigger (not read inside the effect).
  useEffect(() => {
    let cancelled = false;

    if (!overviewVisible) {
      setPassport(null);
      setPassportError(null);
      return;
    }

    void getIntegrityPassport(task.id)
      .then((result) => {
        if (!cancelled) {
          setPassport(result);
          setPassportError(null);
        }
      })
      .catch((error) => {
        if (!cancelled) setPassportError(errorMessage(error));
      });

    return () => {
      cancelled = true;
    };
  }, [overviewVisible, task.id, task.status]);

  const loadMoreSegments = useCallback(async () => {
    if (!segmentsCursor) return;
    try {
      const result = await listSegmentsPage({
        taskId: task.id,
        cursor: segmentsCursor,
        pageSize: 100,
      });
      setSegments((current) => mergeById(current, result.items));
      setSegmentsCursor(result.nextCursor);
      setSegmentError(null);
    } catch (error) {
      setSegmentError(errorMessage(error));
    }
  }, [segmentsCursor, task.id]);

  const loadMoreHlsSegments = useCallback(async () => {
    if (!hlsSegmentsCursor) return;
    try {
      const result = await listHlsSegmentsPage({
        taskId: task.id,
        cursor: hlsSegmentsCursor,
        pageSize: 100,
      });
      setHlsSegments((current) => mergeById(current, result.items));
      setHlsSegmentsCursor(result.nextCursor);
      setHlsSegmentError(null);
    } catch (error) {
      setHlsSegmentError(errorMessage(error));
    }
  }, [hlsSegmentsCursor, task.id]);

  const loadMoreDashSegments = useCallback(async () => {
    if (!dashSegmentsCursor) return;
    try {
      const result = await listDashSegmentsPage({
        taskId: task.id,
        cursor: dashSegmentsCursor,
        pageSize: 100,
      });
      setDashSegments((current) => mergeById(current, result.items));
      setDashSegmentsCursor(result.nextCursor);
      setDashSegmentError(null);
    } catch (error) {
      setDashSegmentError(errorMessage(error));
    }
  }, [dashSegmentsCursor, task.id]);

  const loadMoreEvents = useCallback(async () => {
    if (!eventsCursor) return;
    try {
      const result = await listTaskEventsPage({
        taskId: task.id,
        cursor: eventsCursor,
        pageSize: 100,
      });
      setEvents((current) => mergeById(current, result.items));
      setEventsCursor(result.nextCursor);
      setEventsError(null);
    } catch (error) {
      setEventsError(errorMessage(error));
    }
  }, [eventsCursor, task.id]);

  const loadMoreRequests = useCallback(async () => {
    if (!requestsCursor) return;
    try {
      const result = await listTaskRequestsPage({
        taskId: task.id,
        cursor: requestsCursor,
        pageSize: 100,
      });
      setRequests((current) => mergeById(current, result.items));
      setRequestsCursor(result.nextCursor);
      setRequestsError(null);
    } catch (error) {
      setRequestsError(errorMessage(error));
    }
  }, [requestsCursor, task.id]);

  return {
    segments,
    segmentsCursor,
    segmentError,
    hlsSegments,
    hlsSegmentsCursor,
    hlsSegmentError,
    dashSegments,
    dashSegmentsCursor,
    dashSegmentError,
    events,
    eventsCursor,
    eventsError,
    requests,
    requestsCursor,
    requestsError,
    torrentSnapshot,
    torrentSnapshotError,
    segmentSummary,
    segmentSummaryError,
    ftpSftpEvents,
    sftpKnownHosts,
    passport,
    passportError,
    loadMoreSegments,
    loadMoreHlsSegments,
    loadMoreDashSegments,
    loadMoreEvents,
    loadMoreRequests,
  };
}
