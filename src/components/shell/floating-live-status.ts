export const FLOATING_LIVE_INTERVAL_MS = 5000;
export const FLOATING_LIVE_PERCENT_BUCKET = 10;

export type FloatingLiveMemory = {
  idle: boolean;
  bucket: number;
  at: number;
};

/** Announce idle transitions immediately, and progress at 10% or 5s intervals. */
export function shouldAnnounceFloatingStatus(
  idle: boolean,
  percent: number,
  now: number,
  last: FloatingLiveMemory | null,
): { announce: boolean; next: FloatingLiveMemory } {
  const bucket = idle ? -1 : Math.floor(percent / FLOATING_LIVE_PERCENT_BUCKET);
  const next = { idle, bucket, at: now };
  if (!last) return { announce: true, next };
  if (idle !== last.idle) return { announce: true, next };
  if (idle) return { announce: false, next: { ...last, idle: true, bucket: -1 } };
  if (bucket !== last.bucket || now - last.at >= FLOATING_LIVE_INTERVAL_MS) {
    return { announce: true, next };
  }
  return { announce: false, next: last };
}
