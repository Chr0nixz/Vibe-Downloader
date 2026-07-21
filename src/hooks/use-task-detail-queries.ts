import { useCallback, useEffect, useState } from "react";
import type {
  DashSegmentView,
  HlsSegmentView,
  RequestDiagnostic,
  SegmentSummary,
  SftpKnownHost,
  TaskEvent,
  TorrentRuntimeSnapshot,
} from "@/generated/bindings";
import { errorMessage } from "@/lib/errors";
import { isDashProtocol, isFtpSftpProtocol, isHlsProtocol, isTorrentProtocol } from "@/lib/task-diagnostics";
import {
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

export type TaskDetailDiagSubTab = "segments" | "requests";

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
export function useTaskDetailQueries(options: { task: Task; activeTab: string; diagSubTab: TaskDetailDiagSubTab }) {
  const { task, activeTab, diagSubTab } = options;
  const isTorrentTask = isTorrentProtocol(task.protocol);
  const isHlsTask = isHlsProtocol(task.protocol);
  const isDashTask = isDashProtocol(task.protocol);
  const isFtpSftpTask = isFtpSftpProtocol(task.protocol);
  const segmentsVisible = activeTab === "diagnostics" && diagSubTab === "segments" && !isTorrentTask;
  const overviewVisible = activeTab === "overview";

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
  }, [task.id]);

  // PERF-02: HTTP/FTP/etc work-unit segments — not used for HLS/DASH (dedicated panes) or BT (hidden).
  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;
    let inFlight = false;

    if (!segmentsVisible || usesPlaylistSegments(task.protocol)) {
      setSegments([]);
      setSegmentError(null);
      return;
    }

    const loadSegments = () => {
      if (cancelled || inFlight) return;
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      inFlight = true;
      void listSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
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
          }
        })
        .catch((error) => {
          if (!cancelled) setSegmentError(errorMessage(error));
        })
        .finally(() => {
          inFlight = false;
        });
    };

    const startPolling = () => {
      if (intervalId) return;
      if (task.status === "downloading" || task.status === "retrying") {
        intervalId = setInterval(loadSegments, SEGMENT_REFRESH_MS);
      }
    };

    const stopPolling = () => {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = undefined;
      }
    };

    const onVisibilityChange = () => {
      if (typeof document === "undefined") return;
      if (document.visibilityState === "hidden") {
        stopPolling();
        return;
      }
      loadSegments();
      startPolling();
    };

    loadSegments();
    if (typeof document === "undefined" || document.visibilityState !== "hidden") {
      startPolling();
    }
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    return () => {
      cancelled = true;
      stopPolling();
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
    };
  }, [segmentsVisible, task.protocol, task.id, task.status]);

  // HLS real playlist segments — only while Segments sub-tab is visible.
  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;
    let inFlight = false;

    if (!segmentsVisible || !isHlsTask) {
      setHlsSegments([]);
      setHlsSegmentError(null);
      return;
    }

    const loadHlsSegments = () => {
      if (cancelled || inFlight) return;
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      inFlight = true;
      void listHlsSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setHlsSegments(result.items);
            setHlsSegmentsCursor(result.nextCursor);
            setHlsSegmentError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setHlsSegmentError(errorMessage(error));
        })
        .finally(() => {
          inFlight = false;
        });
    };

    const startPolling = () => {
      if (intervalId) return;
      if (task.status === "downloading" || task.status === "retrying") {
        intervalId = setInterval(loadHlsSegments, SEGMENT_REFRESH_MS);
      }
    };

    const stopPolling = () => {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = undefined;
      }
    };

    const onVisibilityChange = () => {
      if (typeof document === "undefined") return;
      if (document.visibilityState === "hidden") {
        stopPolling();
        return;
      }
      loadHlsSegments();
      startPolling();
    };

    loadHlsSegments();
    if (typeof document === "undefined" || document.visibilityState !== "hidden") {
      startPolling();
    }
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    return () => {
      cancelled = true;
      stopPolling();
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
    };
  }, [segmentsVisible, isHlsTask, task.id, task.status]);

  // DASH real MPD segments — only while Segments sub-tab is visible.
  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;
    let inFlight = false;

    if (!segmentsVisible || !isDashTask) {
      setDashSegments([]);
      setDashSegmentError(null);
      return;
    }

    const loadDashSegments = () => {
      if (cancelled || inFlight) return;
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      inFlight = true;
      void listDashSegmentsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setDashSegments(result.items);
            setDashSegmentsCursor(result.nextCursor);
            setDashSegmentError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setDashSegmentError(errorMessage(error));
        })
        .finally(() => {
          inFlight = false;
        });
    };

    const startPolling = () => {
      if (intervalId) return;
      if (task.status === "downloading" || task.status === "retrying") {
        intervalId = setInterval(loadDashSegments, SEGMENT_REFRESH_MS);
      }
    };

    const stopPolling = () => {
      if (intervalId) {
        clearInterval(intervalId);
        intervalId = undefined;
      }
    };

    const onVisibilityChange = () => {
      if (typeof document === "undefined") return;
      if (document.visibilityState === "hidden") {
        stopPolling();
        return;
      }
      loadDashSegments();
      startPolling();
    };

    loadDashSegments();
    if (typeof document === "undefined" || document.visibilityState !== "hidden") {
      startPolling();
    }
    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    return () => {
      cancelled = true;
      stopPolling();
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
    };
  }, [segmentsVisible, isDashTask, task.id, task.status]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (activeTab !== "diagnostics" || diagSubTab !== "requests") {
      setRequests([]);
      setRequestsError(null);
      return;
    }

    const loadRequests = () => {
      void listTaskRequestsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setRequests(result.items);
            setRequestsCursor(result.nextCursor);
            setRequestsError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setRequestsError(errorMessage(error));
        });
    };

    loadRequests();

    if (isLiveStatus(task.status)) {
      intervalId = setInterval(loadRequests, DETAIL_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, diagSubTab, task.id, task.status]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (!isTorrentTask || activeTab !== "overview") {
      setTorrentSnapshot(null);
      setTorrentSnapshotError(null);
      return;
    }

    const loadSnapshot = () => {
      void getTorrentRuntimeSnapshot(task.id)
        .then((snapshot) => {
          if (!cancelled) {
            setTorrentSnapshot(snapshot);
            setTorrentSnapshotError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setTorrentSnapshotError(errorMessage(error));
        });
    };

    loadSnapshot();

    if (isLiveStatus(task.status)) {
      intervalId = setInterval(loadSnapshot, DETAIL_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, isTorrentTask, task.id, task.status]);

  // FTP/SFTP Overview: segment summary + recent events (acceleration) + SFTP known hosts.
  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (!overviewVisible || !isFtpSftpTask) {
      setSegmentSummary(null);
      setSegmentSummaryError(null);
      setFtpSftpEvents([]);
      setSftpKnownHosts([]);
      return;
    }

    const loadOverviewExtras = () => {
      void getSegmentSummary(task.id)
        .then((summary) => {
          if (!cancelled) {
            setSegmentSummary(summary);
            setSegmentSummaryError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setSegmentSummaryError(errorMessage(error));
        });

      void listTaskEventsPage({ taskId: task.id, cursor: null, pageSize: 50 })
        .then((result) => {
          if (!cancelled) setFtpSftpEvents(result.items);
        })
        .catch(() => {
          if (!cancelled) setFtpSftpEvents([]);
        });

      if (task.protocol === "sftp") {
        void listSftpKnownHosts()
          .then((hosts) => {
            if (!cancelled) setSftpKnownHosts(hosts);
          })
          .catch(() => {
            if (!cancelled) setSftpKnownHosts([]);
          });
      } else if (!cancelled) {
        setSftpKnownHosts([]);
      }
    };

    loadOverviewExtras();

    if (isLiveStatus(task.status)) {
      intervalId = setInterval(loadOverviewExtras, DETAIL_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [overviewVisible, isFtpSftpTask, task.id, task.protocol, task.status]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: ReturnType<typeof setInterval> | undefined;

    if (activeTab !== "logs") {
      setEvents([]);
      setEventsError(null);
      return;
    }

    const loadEvents = () => {
      void listTaskEventsPage({ taskId: task.id, cursor: null, pageSize: 100 })
        .then((result) => {
          if (!cancelled) {
            setEvents(result.items);
            setEventsCursor(result.nextCursor);
            setEventsError(null);
          }
        })
        .catch((error) => {
          if (!cancelled) setEventsError(errorMessage(error));
        });
    };

    loadEvents();

    if (isLiveStatus(task.status)) {
      intervalId = setInterval(loadEvents, DETAIL_REFRESH_MS);
    }

    return () => {
      cancelled = true;
      if (intervalId) clearInterval(intervalId);
    };
  }, [activeTab, task.id, task.status]);

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
    loadMoreSegments,
    loadMoreHlsSegments,
    loadMoreDashSegments,
    loadMoreEvents,
    loadMoreRequests,
  };
}
