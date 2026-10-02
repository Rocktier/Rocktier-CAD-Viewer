/** Number formatting shared by the status bar and the measure overlay. */

/** Not-a-number and infinities must never reach the status bar. */
const DASH = "—";

/**
 * Unit suffix per DXF $INSUNITS code.  Only the codes CAD drawings actually
 * carry are labelled; 0 (and any unrecognised code) means unitless — no suffix,
 * like AutoCAD itself plots then.
 */
const INSUNITS_SUFFIX: Record<number, string> = { 1: "in", 2: "ft", 4: "mm", 5: "cm", 6: "m" };

/** Suffix for the drawing's declared unit ("" when unitless / unknown). */
export function unitLabel(insunits: number | undefined): string {
  return (insunits !== undefined && INSUNITS_SUFFIX[insunits]) || "";
}

/** Append the unit, separated by a space ("12.34 mm"); bare when unitless. */
function withUnit(v: string, unit: string): string {
  return unit ? `${v} ${unit}` : v;
}

export function formatCoord(v: number, unit = ""): string {
  if (!Number.isFinite(v)) return DASH;
  const a = Math.abs(v);
  const s =
    a >= 100000 ? v.toFixed(0)
    : a >= 1000 ? v.toFixed(1)
    : a >= 1 ? v.toFixed(2)
    : v.toFixed(3);
  return withUnit(s, unit);
}

export function formatDist(d: number, unit = ""): string {
  if (!Number.isFinite(d)) return DASH;
  const a = Math.abs(d);
  let s: string;
  if (a === 0) s = "0";
  else if (a >= 100000) s = d.toFixed(0);
  else if (a >= 1000) s = d.toFixed(1);
  else if (a >= 1) s = d.toFixed(2);
  else if (a >= 0.01) s = d.toFixed(3);
  else s = d.toExponential(2);
  return withUnit(s, unit);
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
