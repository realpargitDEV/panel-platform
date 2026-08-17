import { describe, expect, it } from 'vitest';

import { uptimeSeconds } from './RunningPanel';

describe('uptimeSeconds', () => {
  const now = new Date('2026-08-16T12:00:00Z');

  it('is absent when the core did not say when the run started', () => {
    expect(uptimeSeconds(null, now)).toBeNull();
  });

  it('is absent rather than NaN for a timestamp it cannot read', () => {
    expect(uptimeSeconds('not a date', now)).toBeNull();
  });

  it('counts from the start of the run', () => {
    expect(uptimeSeconds('2026-08-16T11:45:00Z', now)).toBe(900);
  });

  /**
   * A window whose clock is a little behind the core's must not render a
   * negative uptime. Zero is the honest floor.
   */
  it('never goes negative when the clocks disagree', () => {
    expect(uptimeSeconds('2026-08-16T12:00:30Z', now)).toBe(0);
  });
});
