/**
 * Turning machine values into something a person reads.
 *
 * Every one of these is pure and tested, because they are the functions that
 * quietly go wrong at a boundary — a duration of exactly a minute, a timestamp
 * from the future because two clocks disagree, a byte count of zero that should
 * read "0 B" and not "NaN".
 */

/** `1.4 GB`. Binary units, because that is what the core measures in. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal below ten, none above: "9.4 GB" is useful, "94.3 GB" is noise.
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/** `2d 4h`, `4h 12m`, `12m`, `48s`. At most two units. */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—';
  const whole = Math.floor(seconds);
  if (whole < 60) return `${whole}s`;

  const minutes = Math.floor(whole / 60);
  if (minutes < 60) return `${minutes}m`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    const rest = minutes % 60;
    return rest > 0 ? `${hours}h ${rest}m` : `${hours}h`;
  }

  const days = Math.floor(hours / 24);
  const rest = hours % 24;
  return rest > 0 ? `${days}d ${rest}h` : `${days}d`;
}

/** `340ms`, `2.4s`, `1m 5s`. What a deployment took. */
export function formatElapsed(milliseconds: number | null): string {
  if (milliseconds === null || !Number.isFinite(milliseconds) || milliseconds < 0) return '—';
  if (milliseconds < 1000) return `${Math.round(milliseconds)}ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1000).toFixed(1)}s`;
  return formatDuration(milliseconds / 1000);
}

/**
 * `just now`, `4m ago`, `yesterday`, or a date once it stops being useful to
 * count.
 *
 * A timestamp slightly in the future reads as "just now" rather than "in -3
 * seconds": the two clocks involved are the database's and the window's, and
 * they do not have to agree to the second.
 */
export function formatRelative(iso: string | null, now: Date = new Date()): string {
  if (!iso) return '—';
  const then = parseTimestamp(iso);
  if (!then) return '—';

  const seconds = (now.getTime() - then.getTime()) / 1000;
  if (seconds < 45) return 'just now';
  if (seconds < 90) return '1m ago';

  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  if (hours < 48) return 'yesterday';

  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;

  return then.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

/** `14:32:08` on the given day, for a log or an event list. */
export function formatTimestamp(iso: string | null): string {
  const parsed = parseTimestamp(iso);
  if (!parsed) return '—';
  return parsed.toLocaleString(undefined, {
    day: 'numeric',
    month: 'short',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

/**
 * How long something has been running, from when it started.
 *
 * Null when it has not started, so a caller shows "—" rather than a duration
 * counted from the epoch.
 */
export function uptimeSeconds(startedAt: string | null, now: Date = new Date()): number | null {
  const started = parseTimestamp(startedAt);
  if (!started) return null;
  return Math.max(0, (now.getTime() - started.getTime()) / 1000);
}

/**
 * SQLite writes `2026-08-01 19:04:11`, which `Date` treats as local time on
 * some engines and refuses on others. Normalising to an ISO instant is what
 * makes the relative times correct rather than eleven hours out.
 */
export function parseTimestamp(value: string | null | undefined): Date | null {
  if (!value) return null;

  const normalised =
    value.includes('T') || value.includes('Z') ? value : `${value.replace(' ', 'T')}Z`;
  const parsed = new Date(normalised);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

/** A percentage of a total, guarding the zero-total case. */
export function percentOf(used: number, total: number): number {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, (used / total) * 100));
}

/**
 * The last component of a path, in either slash.
 *
 * Windows and Linux both appear here: the core reports whatever the host uses,
 * and a display that only understands one of them shows the whole path where a
 * folder name belongs.
 */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return cut < 0 ? trimmed : trimmed.slice(cut + 1);
}
