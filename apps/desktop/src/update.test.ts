import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { UpdateCheck } from './api';
import {
  canStart,
  CHECK_INTERVAL_MS,
  checkFailureMessage,
  controlsFor,
  createUpdateStore,
  describeScreen,
  failureMessage,
  formatBytes,
  formatRate,
  formatRemaining,
  canClose,
  idle,
  isWorking,
  rateBetween,
  screenCaption,
  screenFor,
  screenPercent,
  secondsRemaining,
  smoothRate,
  isBusy,
  phaseFor,
  progressCaption,
  progressPercent,
  type InstallPhase,
  type UpdateBackend,
  type UpdateScreen,
} from './update';

const available: UpdateCheck = {
  state: 'available',
  currentVersion: '0.1.3',
  newVersion: '0.1.4',
  notes: 'Fixes.',
  publishedAt: null,
  downloadUrl: 'https://github.com/x/y/z.exe',
  signature: 'sig',
};

const upToDate: UpdateCheck = { state: 'up_to_date', currentVersion: '0.1.4' };

describe('canStart', () => {
  it('allows a first attempt', () => {
    expect(canStart(idle)).toBe(true);
  });

  it('allows retrying after a failure', () => {
    expect(canStart({ state: 'failed', message: 'no' })).toBe(true);
  });

  /** Two concurrent installs would fight over the same files. */
  it('refuses to start a second install while one is running', () => {
    const running: InstallPhase[] = [
      { state: 'downloading', downloadedBytes: 1, totalBytes: 2 },
      { state: 'verifying' },
      { state: 'installing' },
      { state: 'restarting' },
    ];
    for (const phase of running) {
      expect(canStart(phase), phase.state).toBe(false);
      expect(isBusy(phase), phase.state).toBe(true);
    }
  });

  it('refuses once installed, because what is needed is a restart', () => {
    expect(canStart({ state: 'installed' })).toBe(false);
    // Not busy, though: nothing is running, so a check may resume.
    expect(isBusy({ state: 'installed' })).toBe(false);
  });
});

describe('progressPercent', () => {
  it('reports the fraction downloaded', () => {
    expect(progressPercent({ state: 'downloading', downloadedBytes: 50, totalBytes: 200 })).toBe(
      25,
    );
  });

  /** A bar stuck at 0% while bytes arrive reads as a hang. */
  it('has no percentage when the server sent no length', () => {
    expect(progressPercent({ state: 'downloading', downloadedBytes: 50, totalBytes: null })).toBe(
      null,
    );
  });

  it('never exceeds 100, whatever the server claimed the length was', () => {
    expect(progressPercent({ state: 'downloading', downloadedBytes: 300, totalBytes: 200 })).toBe(
      100,
    );
  });

  it('is full once the bytes are in and the installer is running', () => {
    expect(progressPercent({ state: 'installing' })).toBe(100);
    expect(progressPercent({ state: 'restarting' })).toBe(100);
    expect(progressPercent(idle)).toBe(null);
  });
});

describe('formatBytes', () => {
  it('reads as a person would write it', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1024 * 1024 * 3.5)).toBe('3.5 MB');
    expect(formatBytes(1024 * 1024 * 30)).toBe('30 MB');
  });
});

describe('progressCaption', () => {
  it('says how much of how much', () => {
    expect(
      progressCaption({
        state: 'downloading',
        downloadedBytes: 1024 * 1024,
        totalBytes: 1024 * 1024 * 4,
      }),
    ).toBe('1.0 MB of 4.0 MB');
  });

  it('says what has arrived when there is no total', () => {
    expect(progressCaption({ state: 'downloading', downloadedBytes: 2048, totalBytes: null })).toBe(
      '2.0 KB downloaded',
    );
  });

  it('names the verification step, because that is what protects the user', () => {
    expect(progressCaption({ state: 'verifying' })).toBe(
      'Checking the signature of the downloaded file',
    );
  });
});

describe('failureMessage', () => {
  /** The .deb explanation is a finished sentence and must not be re-wrapped. */
  it('passes a complete sentence through unchanged', () => {
    const deb =
      'This copy was installed from the .deb package, which apt owns and the ' +
      'application cannot replace by itself.';

    expect(failureMessage(deb)).toBe(deb);
  });

  it('wraps a bare fragment so it reads as a sentence', () => {
    expect(failureMessage('connection reset')).toBe(
      'The update could not be installed: connection reset',
    );
  });

  it('reads a message off an Error', () => {
    expect(failureMessage(new Error('signature mismatch'))).toBe(
      'The update could not be installed: signature mismatch',
    );
  });

  /** Tauri rejects with a plain object carrying `message`. */
  it('reads a message off a rejected command object', () => {
    expect(failureMessage({ message: 'no update to install.' })).toBe('no update to install.');
  });

  /** Never show an empty banner that says nothing went wrong and nothing worked. */
  it('says something even when the error carries nothing', () => {
    for (const nothing of [undefined, null, '', '   ', {}]) {
      expect(failureMessage(nothing)).toBe('The update could not be installed.');
    }
  });
});

describe('checkFailureMessage', () => {
  it('blames the check rather than an install that never started', () => {
    expect(checkFailureMessage('connection reset')).toBe(
      'Could not check for updates: connection reset',
    );
    expect(checkFailureMessage({})).toBe('Could not check for updates.');
  });
});

describe('phaseFor', () => {
  it('carries the byte counts through', () => {
    expect(phaseFor({ phase: 'downloading', downloadedBytes: 5, totalBytes: 10 })).toEqual({
      state: 'downloading',
      downloadedBytes: 5,
      totalBytes: 10,
    });
    expect(phaseFor({ phase: 'verifying', downloadedBytes: 0, totalBytes: null })).toEqual({
      state: 'verifying',
    });
  });
});

/**
 * A promise the test resolves by hand.
 *
 * A plain `let resolve: (() => void) | null` assigned inside the executor is
 * narrowed to `never` by TypeScript's control-flow analysis, which is how this
 * ends up as a helper rather than three lines at each call site.
 */
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe('the update store', () => {
  /** A backend whose promises the test resolves by hand. */
  function backend(overrides: Partial<UpdateBackend> = {}) {
    const handlers: Array<(event: never) => void> = [];
    const calls = { check: 0, install: 0, unlisten: 0 };
    const base: UpdateBackend = {
      check: async () => upToDate,
      install: async () => {},
      onProgress: async (handler) => {
        handlers.push(handler as (event: never) => void);
        return () => {
          calls.unlisten += 1;
        };
      },
    };
    // Counted around the override, not inside the default, so a test that
    // supplies its own `install` still gets an accurate count.
    const merged: UpdateBackend = { ...base, ...overrides };
    return {
      backend: {
        check: () => {
          calls.check += 1;
          return merged.check();
        },
        install: () => {
          calls.install += 1;
          return merged.install();
        },
        onProgress: merged.onProgress,
      },
      calls,
      handlers,
    };
  }

  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('checks once on start and then on the interval', async () => {
    const { backend: b, calls } = backend();
    const store = createUpdateStore(b);

    store.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.check).toBe(1);
    expect(store.getState().check).toEqual(upToDate);

    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS);
    expect(calls.check).toBe(2);

    store.stop();
    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS * 3);
    expect(calls.check).toBe(2);
  });

  it('starts only one timer however many times it is started', async () => {
    const { backend: b, calls } = backend();
    const store = createUpdateStore(b);

    store.start();
    store.start();
    store.start();
    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS);
    // One immediate check, one from the single interval.
    expect(calls.check).toBe(2);
    store.stop();
  });

  it('does not start a second check while one is in flight', async () => {
    const gate = deferred();
    const { backend: b, calls } = backend({
      check: async () => {
        await gate.promise;
        return upToDate;
      },
    });
    const store = createUpdateStore(b);

    void store.check();
    void store.check();
    void store.check();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getState().checking).toBe(true);

    gate.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.check).toBe(1);
  });

  it('keeps the last answer when a later check fails', async () => {
    let answer: () => Promise<UpdateCheck> = async () => available;
    const { backend: b } = backend({ check: () => answer() });
    const store = createUpdateStore(b);

    await store.check();
    expect(store.getState().check).toEqual(available);

    answer = () => Promise.reject(new Error('offline'));
    await store.check();
    expect(store.getState().check, 'a blip erased an update already offered').toEqual(available);
    expect(store.getState().checkFailure).toBe('Could not check for updates: offline');
  });

  it('renders each progress event the core sends', async () => {
    const running = deferred();
    const { backend: b, handlers } = backend({ install: () => running.promise });
    const store = createUpdateStore(b);

    void store.install();
    await vi.advanceTimersByTimeAsync(0);
    // Intent only. The phase stays idle until the updater reports a byte —
    // moving it to `downloading` here would be a stage nothing had reached.
    expect(store.getState().accepted).toBe(true);
    expect(store.getState().phase.state).toBe('idle');

    handlers[0]?.({ phase: 'downloading', downloadedBytes: 512, totalBytes: 1024 } as never);
    expect(store.getState().phase).toEqual({
      state: 'downloading',
      downloadedBytes: 512,
      totalBytes: 1024,
    });

    handlers[0]?.({ phase: 'verifying', downloadedBytes: 0, totalBytes: null } as never);
    expect(store.getState().phase.state).toBe('verifying');

    running.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getState().phase.state).toBe('installed');
  });

  /** The requirement the whole store exists for. */
  it('refuses a second install while one is running', async () => {
    const running = deferred();
    const { backend: b, calls } = backend({ install: () => running.promise });
    const store = createUpdateStore(b);

    void store.install();
    await vi.advanceTimersByTimeAsync(0);
    void store.install();
    void store.install();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.install).toBe(1);

    running.resolve();
    await vi.advanceTimersByTimeAsync(0);
  });

  it('does not check while an install is running', async () => {
    const running = deferred();
    const { backend: b, calls } = backend({ install: () => running.promise });
    const store = createUpdateStore(b);

    void store.install();
    await vi.advanceTimersByTimeAsync(0);

    await store.check();
    expect(calls.check, 'a periodic check ran during an install').toBe(0);

    running.resolve();
    await vi.advanceTimersByTimeAsync(0);
  });

  it('reports a failed install and allows another attempt', async () => {
    const { backend: b } = backend({
      install: async () => {
        throw new Error('connection reset');
      },
    });
    const store = createUpdateStore(b);

    await store.install();
    expect(store.getState().phase).toEqual({
      state: 'failed',
      message: 'The update could not be installed: connection reset',
    });
    expect(canStart(store.getState().phase)).toBe(true);
  });

  it('tells its subscribers when anything changes', async () => {
    const { backend: b } = backend();
    const store = createUpdateStore(b);
    const seen: string[] = [];
    const unsubscribe = store.subscribe(() => seen.push(store.getState().phase.state));

    await store.check();
    expect(seen.length).toBeGreaterThan(0);

    unsubscribe();
    const before = seen.length;
    await store.check();
    expect(seen.length).toBe(before);
  });
});

describe('download rate and time remaining', () => {
  it('measures bytes per second between two samples', () => {
    expect(rateBetween({ at: 0, bytes: 0 }, { at: 1000, bytes: 500_000 })).toBe(500_000);
    expect(rateBetween({ at: 1000, bytes: 500_000 }, { at: 3000, bytes: 1_500_000 })).toBe(500_000);
  });

  /** A wrong figure is worse than none: a user watching "2 seconds remaining"
   *  for a minute stops trusting the whole window. */
  it('refuses to guess when the samples cannot support an answer', () => {
    expect(rateBetween({ at: 500, bytes: 10 }, { at: 500, bytes: 20 })).toBeNull();
    expect(rateBetween({ at: 0, bytes: 100 }, { at: 1000, bytes: 40 })).toBeNull();
  });

  it('takes the first sample as the rate, then blends', () => {
    expect(smoothRate(null, 800)).toBe(800);
    expect(smoothRate(1000, 2000, 0.25)).toBe(1250);
  });

  it('computes the time left from the rate', () => {
    expect(secondsRemaining(2_000_000, 10_000_000, 1_000_000)).toBe(8);
    expect(secondsRemaining(10_000_000, 10_000_000, 1_000_000)).toBe(0);
  });

  it('has no estimate without a total or a rate', () => {
    expect(secondsRemaining(10, null, 500)).toBeNull();
    expect(secondsRemaining(10, 100, null)).toBeNull();
    expect(secondsRemaining(10, 100, 0)).toBeNull();
  });

  it('renders a rate only when there is one', () => {
    expect(formatRate(1_500_000)).toBe('1.4 MB/s');
    expect(formatRate(null)).toBeNull();
    expect(formatRate(0)).toBeNull();
  });

  /** Coarse above a minute on purpose: "3 minutes 41 seconds" implies a
   *  precision the estimate does not have. */
  it('renders a duration a person can act on', () => {
    expect(formatRemaining(2)).toBe('a moment left');
    expect(formatRemaining(42)).toBe('42s left');
    expect(formatRemaining(300)).toBe('5 min left');
    expect(formatRemaining(3900)).toBe('1h 5m left');
    expect(formatRemaining(null)).toBeNull();
  });
});

describe('the screen model', () => {
  const base = { check: null, checking: false, checkFailure: null, phase: idle, accepted: false };
  const available = {
    state: 'available' as const,
    currentVersion: '0.1.11',
    newVersion: '0.1.12',
    notes: 'Fixes',
    publishedAt: null,
    downloadUrl: 'https://example.test/x',
    signature: 'sig',
  };

  it('is idle before the first check answers', () => {
    expect(screenFor(base)).toEqual({ state: 'idle' });
  });

  it('shows checking while a check is in flight', () => {
    expect(screenFor({ ...base, checking: true }).state).toBe('checking');
  });

  it('shows the release when one is available', () => {
    expect(screenFor({ ...base, check: available })).toEqual({
      state: 'available',
      version: '0.1.12',
      notes: 'Fixes',
      publishedAt: null,
    });
  });

  it('shows no-update and ahead-of-published distinctly', () => {
    expect(
      screenFor({ ...base, check: { state: 'up_to_date', currentVersion: '0.1.11' } }).state,
    ).toBe('no_update');
    expect(
      screenFor({
        ...base,
        check: { state: 'ahead_of_published', currentVersion: '0.2.0', publishedVersion: '0.1.11' },
      }).state,
    ).toBe('ahead');
  });

  /** Without this the screen would sit on the details page for the first
   *  seconds of every download, until the first byte arrived. */
  it('shows preparing once the install is accepted but no byte has arrived', () => {
    expect(screenFor({ ...base, check: available }, true).state).toBe('preparing');
  });

  it('follows the install through every stage', () => {
    const stage = (phase: typeof base.phase) =>
      screenFor({ ...base, check: available, phase }).state;

    expect(stage({ state: 'downloading', downloadedBytes: 1, totalBytes: 2 })).toBe('downloading');
    expect(stage({ state: 'verifying' })).toBe('verifying');
    expect(stage({ state: 'installing' })).toBe('installing');
    expect(stage({ state: 'restarting' })).toBe('restart_required');
    expect(stage({ state: 'installed' })).toBe('completed');
  });

  /** Once bytes are moving, what the last check said is history. */
  it('lets the install outrank the check', () => {
    const screen = screenFor({
      ...base,
      checking: true,
      check: available,
      phase: { state: 'downloading', downloadedBytes: 5, totalBytes: 10 },
    });
    expect(screen.state).toBe('downloading');
  });

  it('offers retry on a failed install only while there is something to install', () => {
    const failed = { state: 'failed' as const, message: 'boom' };
    expect(screenFor({ ...base, check: available, phase: failed })).toMatchObject({
      state: 'failed',
      canRetry: true,
    });
    expect(screenFor({ ...base, phase: failed })).toMatchObject({ canRetry: false });
  });

  /** A failed check is recovered by checking again, never by installing. */
  it('does not offer an install retry for a failed check', () => {
    expect(screenFor({ ...base, checkFailure: 'no network' })).toEqual({
      state: 'failed',
      message: 'no network',
      canRetry: false,
    });
  });

  it('treats a skipped version as nothing to show', () => {
    expect(screenFor({ ...base, check: { state: 'skipped', skippedVersion: '0.1.9' } }).state).toBe(
      'idle',
    );
  });
});

describe('what the controls may do', () => {
  /** Closing during a download costs the download; during an install it can
   *  leave a half-written application. */
  it('forbids closing while work is in flight', () => {
    for (const state of ['preparing', 'downloading', 'verifying', 'installing'] as const) {
      expect(canClose({ state } as never)).toBe(false);
    }
  });

  it('allows closing everywhere else', () => {
    for (const state of ['idle', 'checking', 'no_update', 'available', 'completed'] as const) {
      expect(canClose({ state } as never)).toBe(true);
    }
  });

  it('knows which states the user is waiting through', () => {
    expect(isWorking({ state: 'downloading' } as never)).toBe(true);
    expect(isWorking({ state: 'checking' } as never)).toBe(true);
    expect(isWorking({ state: 'available' } as never)).toBe(false);
    expect(isWorking({ state: 'completed' } as never)).toBe(false);
  });
});

describe('install intent', () => {
  /**
   * The lifecycle of `accepted`, which is the whole reason the screen can tell
   * "an update is available" from "an install has been asked for". It is set in
   * one place and cleared in one place; these are the transitions that proves
   * it never gets stuck.
   */
  function lifecycleBackend(install: () => Promise<void>) {
    const handlers: Array<(event: never) => void> = [];
    return {
      handlers,
      backend: {
        check: async () => available,
        install,
        onProgress: async (handler: (event: never) => void) => {
          handlers.push(handler);
          return () => {};
        },
      } as unknown as UpdateBackend,
    };
  }

  it('goes available → accepted → preparing → downloading', async () => {
    const running = deferred();
    const { backend: b, handlers } = lifecycleBackend(() => running.promise);
    const store = createUpdateStore(b);

    await store.check();
    expect(screenFor(store.getState()).state).toBe('available');

    void store.install();
    await Promise.resolve();
    expect(store.getState().accepted).toBe(true);
    expect(screenFor(store.getState()).state).toBe('preparing');

    handlers[0]?.({ phase: 'downloading', downloadedBytes: 10, totalBytes: 100 } as never);
    expect(screenFor(store.getState()).state).toBe('downloading');

    running.resolve();
    await Promise.resolve();
  });

  it('clears the intent when an install fails, so a retry is not stuck preparing', async () => {
    const { backend: b } = lifecycleBackend(async () => {
      throw new Error('connection reset');
    });
    const store = createUpdateStore(b);

    await store.check();
    await store.install();

    expect(store.getState().accepted, 'the intent outlived the request').toBe(false);
    const failed = screenFor(store.getState());
    expect(failed.state).toBe('failed');
    expect(failed).toMatchObject({ canRetry: true });
  });

  it('clears the intent when an install completes', async () => {
    const { backend: b } = lifecycleBackend(async () => {});
    const store = createUpdateStore(b);

    await store.check();
    await store.install();

    expect(store.getState().accepted).toBe(false);
    expect(screenFor(store.getState()).state).toBe('completed');
  });

  it('lets a failed install be retried, and shows preparing again', async () => {
    let attempts = 0;
    const running = deferred();
    const { backend: b } = lifecycleBackend(() => {
      attempts += 1;
      if (attempts === 1) return Promise.reject(new Error('connection reset'));
      return running.promise;
    });
    const store = createUpdateStore(b);

    await store.check();
    await store.install();
    expect(screenFor(store.getState()).state).toBe('failed');

    void store.install();
    await Promise.resolve();
    expect(attempts).toBe(2);
    expect(screenFor(store.getState()).state).toBe('preparing');

    running.resolve();
    await Promise.resolve();
  });

  it('returns to the available screen when a failure is followed by a check', async () => {
    let attempts = 0;
    const { backend: b } = lifecycleBackend(async () => {
      attempts += 1;
      throw new Error('connection reset');
    });
    const store = createUpdateStore(b);

    await store.check();
    await store.install();
    expect(screenFor(store.getState()).state).toBe('failed');

    // A fresh answer supersedes a stale failure; the failed phase must not
    // survive it, or the window would report an install that is no longer
    // being attempted.
    await store.check();
    expect(store.getState().phase.state).toBe('idle');
    expect(store.getState().accepted).toBe(false);
    expect(screenFor(store.getState()).state).toBe('available');
    expect(attempts).toBe(1);
  });

  it('clears the intent when a check reports no update', async () => {
    const { backend: b } = lifecycleBackend(async () => {});
    const store = createUpdateStore(b);
    const answers: UpdateCheck[] = [upToDate];

    const swapped = createUpdateStore({
      ...b,
      check: async () => answers[0] ?? upToDate,
    });
    void store;

    await swapped.check();
    expect(swapped.getState().accepted).toBe(false);
    expect(screenFor(swapped.getState()).state).toBe('no_update');
  });

  it('refuses a second install while the first is only intended', async () => {
    const running = deferred();
    let calls = 0;
    const { backend: b } = lifecycleBackend(() => {
      calls += 1;
      return running.promise;
    });
    const store = createUpdateStore(b);

    await store.check();
    void store.install();
    await Promise.resolve();
    // The phase is still idle here, so a guard that only asked the phase would
    // wave the second one through.
    void store.install();
    await Promise.resolve();
    expect(calls).toBe(1);

    running.resolve();
    await Promise.resolve();
  });

  it('refuses to check while an install is only intended', async () => {
    const running = deferred();
    let checks = 0;
    const { backend: b } = lifecycleBackend(() => running.promise);
    const store = createUpdateStore({
      ...b,
      check: async () => {
        checks += 1;
        return available;
      },
    });

    await store.check();
    expect(checks).toBe(1);

    void store.install();
    await Promise.resolve();
    await store.check();
    expect(checks, 'a check ran between the press and the first byte').toBe(1);

    running.resolve();
    await Promise.resolve();
  });
});

describe('what each screen says', () => {
  /** Every state produces copy; none of it is empty, and none of it lies. */
  it('describes all twelve states', () => {
    const screens: UpdateScreen[] = [
      { state: 'idle' },
      { state: 'checking' },
      { state: 'no_update', currentVersion: '0.1.11' },
      { state: 'ahead', currentVersion: '0.2.0', publishedVersion: '0.1.11' },
      { state: 'available', version: '0.1.12', notes: 'Fixes', publishedAt: null },
      { state: 'preparing' },
      { state: 'downloading', downloadedBytes: 1, totalBytes: 2 },
      { state: 'verifying' },
      { state: 'installing' },
      { state: 'restart_required' },
      { state: 'completed' },
      { state: 'failed', message: 'connection reset', canRetry: true },
    ];

    for (const screen of screens) {
      const copy = describeScreen(screen);
      expect(copy.title.length, screen.state).toBeGreaterThan(0);
      expect(copy.detail.length, screen.state).toBeGreaterThan(0);
    }
  });

  it('names the version in the available and up-to-date screens', () => {
    expect(
      describeScreen({ state: 'available', version: '0.1.12', notes: '', publishedAt: null }).title,
    ).toContain('0.1.12');
    expect(describeScreen({ state: 'no_update', currentVersion: '0.1.11' }).detail).toContain(
      '0.1.11',
    );
  });

  it('separates a failed check from a failed install in the headline', () => {
    const install = describeScreen({ state: 'failed', message: 'x', canRetry: true });
    const checkFailed = describeScreen({ state: 'failed', message: 'x', canRetry: false });
    expect(install.title).toContain('update');
    expect(checkFailed.title).toContain('check');
    expect(install.detail).toBe('x');
  });

  it('says why there is no percentage when the size is unknown', () => {
    const copy = describeScreen({ state: 'downloading', downloadedBytes: 5, totalBytes: null });
    expect(copy.detail).toContain('did not report a size');
  });

  it('gives no percentage and no caption outside a download', () => {
    expect(
      screenPercent({ state: 'downloading', downloadedBytes: 5, totalBytes: null }),
    ).toBeNull();
    expect(screenPercent({ state: 'downloading', downloadedBytes: 5, totalBytes: 10 })).toBe(50);
    expect(screenPercent({ state: 'verifying' })).toBe(100);
    expect(screenPercent({ state: 'available', version: '1', notes: '', publishedAt: null })).toBe(
      null,
    );
    expect(screenCaption({ state: 'verifying' })).toBeNull();
    expect(screenCaption({ state: 'downloading', downloadedBytes: 1024, totalBytes: null })).toBe(
      '1.0 KB downloaded',
    );
  });
});

describe('which controls each screen offers', () => {
  it('offers Install only where there is something to install', () => {
    expect(
      controlsFor({ state: 'available', version: '1', notes: '', publishedAt: null }).install,
    ).toBe(true);
    expect(controlsFor({ state: 'no_update', currentVersion: '1' }).install).toBe(false);
    expect(controlsFor({ state: 'downloading', downloadedBytes: 1, totalBytes: 2 }).install).toBe(
      false,
    );
  });

  it('retries the install after an install failure and the check after a check failure', () => {
    const installFailed = controlsFor({ state: 'failed', message: 'x', canRetry: true });
    expect(installFailed.retryInstall).toBe(true);
    expect(installFailed.check).toBe(true);

    const checkFailed = controlsFor({ state: 'failed', message: 'x', canRetry: false });
    expect(checkFailed.retryInstall, 'offered to retry an install that never started').toBe(false);
    expect(checkFailed.check).toBe(true);
  });

  it('offers Minimise through every wait and nowhere else', () => {
    const waits = ['checking', 'preparing', 'downloading', 'verifying', 'installing'] as const;
    for (const state of waits) {
      expect(controlsFor({ state } as never).minimize, state).toBe(true);
    }
    for (const state of ['idle', 'no_update', 'available', 'completed'] as const) {
      expect(controlsFor({ state } as never).minimize, state).toBe(false);
    }
  });

  it('offers Restart once the install is in place', () => {
    expect(controlsFor({ state: 'restart_required' }).restart).toBe(true);
    expect(controlsFor({ state: 'completed' }).restart).toBe(true);
    expect(controlsFor({ state: 'installing' }).restart).toBe(false);
    expect(
      controlsFor({ state: 'available', version: '1', notes: '', publishedAt: null }).restart,
    ).toBe(false);
  });

  it('disables closing exactly while closing would cost something', () => {
    for (const state of ['preparing', 'downloading', 'verifying', 'installing'] as const) {
      expect(controlsFor({ state } as never).closeEnabled, state).toBe(false);
    }
    expect(controlsFor({ state: 'checking' }).closeEnabled).toBe(true);
    expect(controlsFor({ state: 'completed' }).closeEnabled).toBe(true);
  });

  it('never offers a pause or a cancel', () => {
    // Not a style preference: the updater cannot honour either, and the shape
    // of this type is what stops one being added without the capability.
    const control = controlsFor({ state: 'downloading', downloadedBytes: 1, totalBytes: 2 });
    expect(Object.keys(control)).not.toContain('pause');
    expect(Object.keys(control)).not.toContain('cancel');
  });
});

describe('the surface it replaced', () => {
  /**
   * The old banner-and-panel updater is gone, not merely unused.
   *
   * A dead component that still compiles is one import away from coming back,
   * and two surfaces drawing the same install from the same store is the bug
   * the manager was built to end. Asserted against the tree rather than
   * described in a comment, so deleting it stays deleted.
   */
  it('leaves no second updater interface behind', () => {
    const root = fileURLToPath(new URL('.', import.meta.url));
    expect(existsSync(join(root, 'components/UpdateProgress.tsx'))).toBe(false);

    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
          walk(full);
          continue;
        }
        if (!/\.tsx?$/.test(entry.name)) continue;
        // This file names the thing it is looking for, so it would find itself.
        if (full === fileURLToPath(import.meta.url)) continue;
        const source = readFileSync(full, 'utf8');
        // Narrow on purpose: `UpdateProgressEvent` is the core's payload type
        // and `onUpdateProgress` is how it is subscribed to — both stay. What
        // must not come back is the component, or any use of it.
        if (/components\/UpdateProgress|<UpdateProgress[\s/>]/.test(source)) offenders.push(full);
      }
    };
    walk(root);
    expect(offenders).toEqual([]);
  });
});
