export type LogLevel = "debug" | "info" | "warn" | "error";

export interface Logger {
  debug: (...args: unknown[]) => void;
  info: (...args: unknown[]) => void;
  warn: (...args: unknown[]) => void;
  error: (...args: unknown[]) => void;
  withContext: (ctx: Record<string, unknown>) => Logger;
}

export function createLogger(namespace: string): Logger {
  const prefix = `[vibe:${namespace}]`;

  const logger: Logger = {
    debug: (...args: unknown[]) => {
      if (import.meta.env.DEV) console.debug(prefix, ...args);
    },
    info: (...args: unknown[]) => console.info(prefix, ...args),
    warn: (...args: unknown[]) => console.warn(prefix, ...args),
    error: (...args: unknown[]) => console.error(prefix, ...args),
    withContext: (ctx: Record<string, unknown>) =>
      createLogger(`${namespace}:${formatContext(ctx)}`),
  };

  return logger;
}

function formatContext(ctx: Record<string, unknown>): string {
  return Object.entries(ctx)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join(",");
}

export function installGlobalErrorLogging(logger: Logger): void {
  window.addEventListener("error", (event) => {
    logger.error("uncaught error", event.error ?? event.message);
  });
  window.addEventListener("unhandledrejection", (event) => {
    logger.error("unhandled rejection", event.reason);
  });
}
