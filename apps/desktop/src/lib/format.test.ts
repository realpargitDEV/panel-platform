import { describe, expect, it } from 'vitest';

import {
  baseName,
  formatBytes,
  formatDuration,
  formatElapsed,
  formatRelative,
  parseTimestamp,
  percentOf,
  uptimeSeconds,
} from './format';

describe('bytes', () => {
  it('shows plain bytes below a kilobyte', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(999)).toBe('999 B');
  });

  it('steps up through the units', () => {
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(3 * 1024 ** 3)).toBe('3.0 GB');
    expect(formatBytes(2 * 1024 ** 4)).toBe('2.0 TB');
  });

  it('drops the decimal once the number is big enough not to need it', () => {
    expect(formatBytes(94 * 1024 ** 3)).toBe('94 GB');
  });

  it('refuses to render nonsense as a number', () => {
    expect(formatBytes(Number.NaN)).toBe('—');
    expect(formatBytes(-5)).toBe('—');
  });
});

describe('durations', () => {
  it('counts seconds, then minutes, then hours, then days', () => {
    expect(formatDuration(48)).toBe('48s');
    expect(formatDuration(12 * 60)).toBe('12m');
    expect(formatDuration(4 * 3600 + 12 * 60)).toBe('4h 12m');
    expect(formatDuration(2 * 86400 + 4 * 3600)).toBe('2d 4h');
  });

  it('drops the second unit when it is zero', () => {
    expect(formatDuration(3600)).toBe('1h');
    expect(formatDuration(2 * 86400)).toBe('2d');
  });

  it('handles the exact boundaries', () => {
    expect(formatDuration(59)).toBe('59s');
    expect(formatDuration(60)).toBe('1m');
    expect(formatDuration(3599)).toBe('59m');
  });

  it('shows nothing rather than a negative duration', () => {
    expect(formatDuration(-1)).toBe('—');
  });
});

describe('elapsed time', () => {
  it('uses milliseconds, seconds, then the duration format', () => {
    expect(formatElapsed(340)).toBe('340ms');
    expect(formatElapsed(2400)).toBe('2.4s');
    expect(formatElapsed(65_000)).toBe('1m');
  });

  it('shows a dash when nothing was recorded', () => {
    expect(formatElapsed(null)).toBe('—');
  });
});

describe('relative time', () => {
  const now = new Date('2026-08-01T12:00:00Z');

  it('counts up through the units', () => {
    expect(formatRelative('2026-08-01T11:59:50Z', now)).toBe('just now');
    expect(formatRelative('2026-08-01T11:56:00Z', now)).toBe('4m ago');
    expect(formatRelative('2026-08-01T09:00:00Z', now)).toBe('3h ago');
    expect(formatRelative('2026-07-31T10:00:00Z', now)).toBe('yesterday');
    expect(formatRelative('2026-07-25T12:00:00Z', now)).toBe('7d ago');
  });

  it('falls back to a date once counting stops being useful', () => {
    expect(formatRelative('2025-01-05T12:00:00Z', now)).toMatch(/2025/);
  });

  it('treats a timestamp from the future as now', () => {
    // The database's clock and the window's do not have to agree to the
    // second, and "in -3 seconds" is never the right thing to show.
    expect(formatRelative('2026-08-01T12:00:05Z', now)).toBe('just now');
  });

  it('shows a dash for a missing timestamp', () => {
    expect(formatRelative(null, now)).toBe('—');
    expect(formatRelative('not a date', now)).toBe('—');
  });
});

describe('parsing the timestamps SQLite writes', () => {
  it('reads a space-separated timestamp as UTC, not local time', () => {
    // Read as local time this is off by the reader's offset, which is how
    // "2 minutes ago" becomes "11 hours ago" on a machine east of Greenwich.
    expect(parseTimestamp('2026-08-01 12:00:00')?.toISOString()).toBe('2026-08-01T12:00:00.000Z');
  });

  it('reads a proper ISO instant unchanged', () => {
    expect(parseTimestamp('2026-08-01T12:00:00Z')?.toISOString()).toBe('2026-08-01T12:00:00.000Z');
  });

  it('returns null for anything it cannot read', () => {
    expect(parseTimestamp(null)).toBeNull();
    expect(parseTimestamp('')).toBeNull();
    expect(parseTimestamp('whenever')).toBeNull();
  });
});

describe('uptime', () => {
  const now = new Date('2026-08-01T12:00:00Z');

  it('measures from the start time', () => {
    expect(uptimeSeconds('2026-08-01T11:00:00Z', now)).toBe(3600);
  });

  it('is null when the project never started', () => {
    expect(uptimeSeconds(null, now)).toBeNull();
  });

  it('never goes negative', () => {
    expect(uptimeSeconds('2026-08-01T12:05:00Z', now)).toBe(0);
  });
});

describe('percentages', () => {
  it('divides and clamps', () => {
    expect(percentOf(50, 200)).toBe(25);
    expect(percentOf(300, 200)).toBe(100);
    expect(percentOf(-5, 200)).toBe(0);
  });

  it('returns zero rather than infinity when there is no total', () => {
    expect(percentOf(5, 0)).toBe(0);
  });
});

describe('path names', () => {
  it('reads both slash styles, because both hosts appear here', () => {
    expect(baseName('C:\\Users\\me\\projects\\api')).toBe('api');
    expect(baseName('/home/me/projects/api')).toBe('api');
  });

  it('ignores a trailing separator', () => {
    expect(baseName('/home/me/api/')).toBe('api');
    expect(baseName('C:\\Users\\me\\api\\')).toBe('api');
  });

  it('returns a bare name unchanged', () => {
    expect(baseName('api')).toBe('api');
  });
});
