/** Number formatting shared by the status bar and the measure overlay. */

/** Not-a-number and infinities must never reach the status bar. */
const DASH = "—";

export function formatCoord(v: number): string {
  if (!Number.isFinite(v)) return DASH;
  const a = Math.abs(v);
  if (a >= 100000) return v.toFixed(0);
  if (a >= 1000) return v.toFixed(1);
  if (a >= 1) return v.toFixed(2);
  return v.toFixed(3);
}

export function formatDist(d: number): string {
  if (!Number.isFinite(d)) return DASH;
  const a = Math.abs(d);
  if (a === 0) return "0";
  if (a >= 100000) return d.toFixed(0);
  if (a >= 1000) return d.toFixed(1);
  if (a >= 1) return d.toFixed(2);
  if (a >= 0.01) return d.toFixed(3);
  return d.toExponential(2);
}

export function formatCount(n: number): string {
  if (!Number.isFinite(n)) return DASH;
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1000) return (n / 1000).toFixed(1) + "k";
  return String(n);
}

/** Format a millisecond duration for the status bar (e.g. "12ms", "1.3s"). */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return DASH;
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}
