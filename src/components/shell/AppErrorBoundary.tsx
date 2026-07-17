import { useCallback } from "react";
import { ErrorBoundary, type FallbackProps } from "react-error-boundary";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

// Defensive fallbacks: the error boundary fires when the app is broken,
// so the i18n bundle may not be initialized. Always have English strings ready.
const FALLBACK = {
  title: "Something went wrong",
  description: "The app hit an unexpected error. Try reloading, or copy the error details to report.",
  reload: "Reload",
  copyError: "Copy error",
  home: "Go home",
} as const;

function ErrorFallback({ error, resetErrorBoundary }: FallbackProps) {
  // t may return the key itself if i18n isn't ready; fall back to English.
  const { t } = useTranslation();
  const title = t("errorBoundary.title", { defaultValue: FALLBACK.title });
  const description = t("errorBoundary.description", { defaultValue: FALLBACK.description });
  const reloadLabel = t("errorBoundary.reload", { defaultValue: FALLBACK.reload });
  const copyLabel = t("errorBoundary.copyError", { defaultValue: FALLBACK.copyError });
  const homeLabel = t("errorBoundary.home", { defaultValue: FALLBACK.home });

  const handleReload = useCallback(() => {
    window.location.reload();
  }, []);

  const handleCopy = useCallback(() => {
    const text = error instanceof Error ? `${error.message}\n\n${error.stack ?? ""}` : String(error);
    navigator.clipboard.writeText(text).catch(() => {});
  }, [error]);

  return (
    <div
      role="alert"
      className="flex h-full w-full flex-col items-center justify-center gap-6 bg-surface-root p-8 text-center"
    >
      <div className="flex h-14 w-14 items-center justify-center rounded-full bg-status-danger/10">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
          className="h-7 w-7 text-status-danger"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
      </div>

      <div className="flex flex-col gap-2">
        <h1 className="text-lg font-semibold text-text-primary">{title}</h1>
        <p className="max-w-md text-sm text-text-secondary">{description}</p>
      </div>

      {error instanceof Error && (
        <pre className="max-h-40 w-full max-w-lg overflow-auto rounded-md border border-border-subtle bg-surface-raised p-3 text-left font-mono text-xs text-text-muted">
          {error.message}
        </pre>
      )}

      <div className="flex items-center gap-3">
        <Button type="button" onClick={handleReload}>
          {reloadLabel}
        </Button>
        <Button type="button" variant="outline" onClick={handleCopy}>
          {copyLabel}
        </Button>
        <Button type="button" variant="outline" onClick={resetErrorBoundary}>
          {homeLabel}
        </Button>
      </div>
    </div>
  );
}

export function AppErrorBoundary({ children }: { children: React.ReactNode }) {
  return (
    <ErrorBoundary
      FallbackComponent={ErrorFallback}
      onError={(error, info) => {
        console.error("[AppErrorBoundary]", error, info.componentStack);
      }}
    >
      {children}
    </ErrorBoundary>
  );
}
