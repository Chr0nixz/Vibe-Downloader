import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import type { ClipboardLinkDetectedPayload } from "@/generated/bindings";
import { useClipboardLinkMonitor } from "@/hooks/use-clipboard-link-monitor";
import { isOwnClipboardWrite } from "@/lib/clipboard-write";
import { sendSystemNotification } from "@/lib/system-notification";
import { sanitizeUrlForDisplay } from "@/lib/utils";
import { useToastStore } from "@/stores/toast-store";

/**
 * UX-43: turns clipboard-monitor detections into a non-modal prompt. A
 * detection never opens the new-download dialog or fires a probe by itself;
 * the toast's "Use link" action is the user's explicit consent, and only
 * then does `onAccept` open the dialog (which probes like a pasted URL).
 *
 * - App-side writes registered in clipboard-write.ts are dropped, so "copy
 *   download URL" cannot bounce back as a detection.
 * - While the window is hidden (minimized or in the tray) nobody can see a
 *   toast before it expires, so the latest detection is held and shown when
 *   the window becomes visible. Only the latest is kept: the clipboard itself
 *   holds one value, and a backlog of stale prompts would just be noise.
 */
export function useClipboardLinkPrompt(onAccept: (payload: ClipboardLinkDetectedPayload) => void): void {
  const { t } = useTranslation();
  const addToast = useToastStore((state) => state.addToast);
  const pendingRef = useRef<ClipboardLinkDetectedPayload | null>(null);
  const lastBackgroundNotificationRef = useRef(0);

  const showPromptRef = useRef<(payload: ClipboardLinkDetectedPayload) => void>(() => {});
  showPromptRef.current = (payload) => {
    const batch = payload.urls.length > 1;
    addToast({
      tone: "info",
      title: batch
        ? t("toast.clipboardLinksDetected", { count: payload.urls.length })
        : t("toast.clipboardLinkDetected"),
      description: batch
        ? t("toast.clipboardLinksDetectedDescription", { count: payload.urls.length })
        : sanitizeUrlForDisplay(payload.primaryUrl),
      action: {
        label: t("toast.useClipboardLink"),
        onClick: () => onAccept(payload),
      },
    });
  };

  useClipboardLinkMonitor((payload) => {
    if (payload.urls.length === 0) return;
    if (isOwnClipboardWrite(payload.urls)) return;
    if (document.visibilityState === "hidden") {
      pendingRef.current = payload;
      // The URL itself may contain a token. Keep the native notification
      // generic and rely on the held in-app toast for the actual preview.
      const now = Date.now();
      if (now - lastBackgroundNotificationRef.current >= 5000) {
        lastBackgroundNotificationRef.current = now;
        void sendSystemNotification(t("toast.clipboardLinkDetected"), t("toast.useClipboardLink"), {
          requestPermission: false,
        });
      }
      return;
    }
    showPromptRef.current(payload);
  });

  useEffect(() => {
    const onVisibilityChange = () => {
      if (document.visibilityState === "hidden") return;
      const pending = pendingRef.current;
      pendingRef.current = null;
      if (pending) showPromptRef.current(pending);
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => document.removeEventListener("visibilitychange", onVisibilityChange);
  }, []);
}
