import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { UpdateCheck } from './api';
import {
  buttonLabel,
  canStart,
  CHECK_INTERVAL_MS,
  checkFailureMessage,
  createUpdateStore,
  describeCheck,
  failureMessage,
  formatBytes,
  formatRate,
  formatRemaining,
  canClose,
  idle,
  isWorking,
  rateBetween,
  screenFor,
  secondsRemaining,
  smoothRate,
  isBusy,
  phaseFor,
  progressCaption,
  progressPercent,
  type InstallPhase,
  type UpdateBackend,
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

describe('buttonLabel', () => {
  it('names the action, then each step, then the next thing to do', () => {
    const labels: Array<[InstallPhase, string]> = [
      [idle, 'Update now'],
      [{ state: 'downloading', downloadedBytes: 0, totalBytes: null }, 'Downloading…'],
      [{ state: 'verifying' }, 'Verifying…'],
      [{ state: 'installing' }, 'Installing…'],
      [{ state: 'restarting' }, 'Restarting…'],
      [{ state: 'installed' }, 'Restart to finish'],
      [{ state: 'failed', message: 'x' }, 'Try again'],
    ];

    for (const [phase, expected] of labels) {
      expect(buttonLabel(phase)).toBe(expected);
    }
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

describe('describeCheck', () => {
  /** The two sentences the interface is required to produce. */
  it('says an update is available only for an available result', () => {
    expect(describeCheck(available)).toBe('Update available — version 0.1.4');
  });

  it("says you're up to date when there is nothing newer", () => {
    expect(describeCheck(upToDate)).toBe("You're up to date (version 0.1.4)");
  });

  it('does not claim an update for a skipped or ahead result', () => {
    const skipped = describeCheck({ state: 'skipped', skippedVersion: '0.2.0' });
    const ahead = describeCheck({
      state: 'ahead_of_published',
      currentVersion: '0.9.0',
      publishedVersion: '0.1.4',
    });
    expect(skipped).not.toContain('Update available');
    expect(ahead).not.toContain('Update available');
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
    expect(store.getState().phase.state).toBe('downloading');

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
  const base = { check: null, checking: false, checkFailure: null, phase: idle };
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
