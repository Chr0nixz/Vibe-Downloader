export interface AppErrorPayload {
  code: string;
  message: string;
  recoverable: boolean;
  actions: string[];
}

export function parseAppError(error: unknown): AppErrorPayload | null {
  if (isAppErrorPayload(error)) return error;
  if (typeof error !== "string") return null;

  try {
    const parsed = JSON.parse(error) as unknown;
    return isAppErrorPayload(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function errorMessage(error: unknown): string {
  const payload = parseAppError(error);
  if (payload) return payload.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

function isAppErrorPayload(value: unknown): value is AppErrorPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<AppErrorPayload>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.recoverable === "boolean" &&
    Array.isArray(candidate.actions) &&
    candidate.actions.every((action) => typeof action === "string")
  );
}
