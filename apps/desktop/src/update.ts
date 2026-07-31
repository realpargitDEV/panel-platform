/**
 * The update prompt's state machine, and the words it shows.
 *
 * Separated from the components because both the banner and the Settings screen
 * offer the same action and must behave identically — and because the parts
 * worth testing are these, not the markup.
 *
 * The store at the bottom is why this file grew: two components each holding
 * their own `useState` meant two independent ideas of whether an update was
 * being installed, and the button in one place stayed enabled while the other
 * was already downloading. There is one update process, so there is one store.
 */
import type { UpdateCheck } from './api';

/** Where an install attempt has got to. */
export type InstallPhase =
  | { state: 'idle' }
  /** Bytes are arriving. `totalBytes` is null when the server sent no length. */
  | { state: 'downloading'; downloadedBytes: number; totalBytes: number | null }
  /** The signature is being checked against the key compiled into the build. */
  | { state: 'verifying' }
  | { state: 'installing' }
  /** Handing over to the new version. */
  | { state: 'restarting' }
  /** Only reachable where the install returns instead of replacing the process. */
  | { state: 'installed' }
  | { state: 'failed'; message: string };

export const idle: InstallPhase = { state: 'idle' };

/** An install may be started only when nothing is already running. */
export function canStart(phase: InstallPhase): boolean {
  return phase.state === 'idle' || phase.state === 'failed';
}

/** True while the update process owns the application. */
export function isBusy(phase: InstallPhase): boolean {
  return !canStart(phase) && phase.state !== 'installed';
}

export function buttonLabel(phase: InstallPhase): string {
  switch (phase.state) {
    case 'downloading':
      return 'Downloading…';
    case 'verifying':
      return 'Verifying…';
    case 'installing':
      return 'Installing…';
    case 'restarting':
      return 'Restarting…';
    case 'installed':
      return 'Restart to finish';
    case 'failed':
      return 'Try again';
    default:
      return 'Update now';
  }
}

/**
 * How far along, 0–100, or `null` when that cannot honestly be said.
 *
 * `null` is not zero: a download whose response carried no `Content-Length` has
 * no percentage, and a bar sitting at 0% while bytes arrive looks stuck. The
 * window renders an indeterminate bar for `null` rather than a wrong number.
 */
export function progressPercent(phase: InstallPhase): number | null {
  if (phase.state !== 'downloading') {
    return phase.state === 'installing' || phase.state === 'restarting' ? 100 : null;
  }
  if (phase.totalBytes === null || phase.totalBytes <= 0) {
    return null;
  }
  const percent = (phase.downloadedBytes / phase.totalBytes) * 100;
  // Clamped: a server that under-reports its length must not produce 118%.
  return Math.max(0, Math.min(100, Math.round(percent)));
}

/** A size a person can read, for the line under the progress bar. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/** The progress line: "12.4 MB of 30.1 MB", or just what has arrived. */
export function progressCaption(phase: InstallPhase): string | null {
  switch (phase.state) {
    case 'downloading':
      return phase.totalBytes === null
        ? `${formatBytes(phase.downloadedBytes)} downloaded`
        : `${formatBytes(phase.downloadedBytes)} of ${formatBytes(phase.totalBytes)}`;
    case 'verifying':
      return 'Checking the signature of the downloaded file';
    case 'installing':
      return 'Running the installer';
    case 'restarting':
      return 'Starting the new version';
    default:
      return null;
  }
}

/**
 * What the check produced, in one line.
 *
 * The two states that matter most — "Update available" and "You're up to date"
 * — are the first two cases, and neither is shown speculatively: `available` is
 * only produced by the core when the published version is strictly greater than
 * the installed one.
 */
export function describeCheck(check: UpdateCheck): string {
  switch (check.state) {
    case 'available':
      return `Update available — version ${check.newVersion}`;
    case 'up_to_date':
      return `You're up to date (version ${check.currentVersion})`;
    case 'skipped':
      return `Version ${check.skippedVersion} was skipped`;
    case 'ahead_of_published':
      return `You are running ${check.currentVersion}, which is newer than the published ${check.publishedVersion}`;
  }
}

/**
 * What to tell the user when an install fails.
 *
 * Tauri's errors arrive as whatever the plugin threw, which for a `.deb`
 * install used to be a complaint about a missing `APPIMAGE` variable — true,
 * and useless. The command now returns a sentence for that case, so anything
 * already ending in a full stop is passed through as written rather than
 * wrapped in a second layer of apology.
 */
export function failureMessage(error: unknown): string {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : typeof error === 'object' && error !== null && 'message' in error
          ? String((error as { message: unknown }).message)
          : '';

  const text = raw.trim();
  if (text === '') {
    return 'The update could not be installed.';
  }
  if (/[.!?]$/.test(text)) {
    return text;
  }
  return `The update could not be installed: ${text}`;
}

/** The same, for a check rather than an install. */
export function checkFailureMessage(error: unknown): string {
  const message = failureMessage(error);
  if (message === 'The update could not be installed.') {
    return 'Could not check for updates.';
  }
  return message.startsWith('The update could not be installed: ')
    ? message.replace('The update could not be installed: ', 'Could not check for updates: ')
    : message;
}

// ------------------------------------------------------------------- the store

/**
 * How often to look while the application is open.
 *
 * Six hours: often enough that a machine left running for a week is not sitting
 * on a version from Monday, rare enough that nobody notices the traffic. The
 * check is one small file over HTTPS.
 */
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export interface UpdateState {
  /** The last answer, or null before the first check finished. */
  check: UpdateCheck | null;
  /** True while a check is in flight, so a second one is not started. */
  checking: boolean;
  /** Why the last check failed. Shown where the user asked for it. */
  checkFailure: string | null;
  phase: InstallPhase;
}

export const initialState: UpdateState = {
  check: null,
  checking: false,
  checkFailure: null,
  phase: idle,
};

/** The `update://progress` payload, as the core emits it. */
export interface UpdateProgressEvent {
  phase: 'downloading' | 'verifying' | 'installing' | 'restarting';
  downloadedBytes: number;
  totalBytes: number | null;
}

/** What the store needs from the outside world. Injected so it can be tested. */
export interface UpdateBackend {
  check: () => Promise<UpdateCheck>;
  install: () => Promise<void>;
  /** Subscribes to progress events; resolves to an unsubscribe. */
  onProgress: (handler: (progress: UpdateProgressEvent) => void) => Promise<() => void>;
}

/** Turn a progress event into the phase the window renders. */
export function phaseFor(event: UpdateProgressEvent): InstallPhase {
  switch (event.phase) {
    case 'downloading':
      return {
        state: 'downloading',
        downloadedBytes: event.downloadedBytes,
        totalBytes: event.totalBytes,
      };
    case 'verifying':
      return { state: 'verifying' };
    case 'installing':
      return { state: 'installing' };
    case 'restarting':
      return { state: 'restarting' };
  }
}

export interface UpdateStore {
  getState: () => UpdateState;
  subscribe: (listener: () => void) => () => void;
  /** Check now. Resolves when the check finishes; never throws. */
  check: () => Promise<void>;
  /** Start the update. Refused while one is already running. */
  install: () => Promise<void>;
  /** Begin checking: once now, then every `CHECK_INTERVAL_MS`. Idempotent. */
  start: () => void;
  /** Stop the periodic check and drop the progress subscription. */
  stop: () => void;
}

/**
 * One update process for the whole window.
 *
 * Single-flight in both directions: a check that is already running is not
 * started again, and an install that is running blocks both a second install
 * and the periodic check — asking the feed a question while the answer to the
 * last one is being installed can only produce a confusing banner. The core
 * enforces the install half again on its own side, because a guard that lives
 * only in the interface is a guard a second window walks straight past.
 */
export function createUpdateStore(backend: UpdateBackend): UpdateStore {
  let state: UpdateState = initialState;
  const listeners = new Set<() => void>();
  let timer: ReturnType<typeof setInterval> | null = null;
  let unlisten: (() => void) | null = null;

  const set = (next: Partial<UpdateState>) => {
    state = { ...state, ...next };
    for (const listener of listeners) {
      listener();
    }
  };

  const check = async () => {
    // Already asking, or busy installing the answer to the last question.
    if (state.checking || isBusy(state.phase)) {
      return;
    }
    set({ checking: true, checkFailure: null });
    try {
      set({ check: await backend.check(), checkFailure: null });
    } catch (error) {
      // The previous result is left alone: a network blip should not erase an
      // update the user has already been told about.
      set({ checkFailure: checkFailureMessage(error) });
    } finally {
      set({ checking: false });
    }
  };

  const install = async () => {
    if (!canStart(state.phase)) {
      return;
    }
    set({ phase: { state: 'downloading', downloadedBytes: 0, totalBytes: null } });

    if (unlisten === null) {
      try {
        unlisten = await backend.onProgress((event) => set({ phase: phaseFor(event) }));
      } catch {
        // No progress events is a worse interface, not a broken one: the
        // install still runs and still reports how it ended.
        unlisten = null;
      }
    }

    try {
      await backend.install();
      // Reached only where the install returns rather than replacing the
      // process — on Windows the installer takes over and nothing after this
      // line runs.
      set({ phase: { state: 'installed' } });
    } catch (error) {
      set({ phase: { state: 'failed', message: failureMessage(error) } });
    }
  };

  return {
    getState: () => state,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    check,
    install,
    start: () => {
      if (timer !== null) {
        return;
      }
      void check();
      timer = setInterval(() => void check(), CHECK_INTERVAL_MS);
    },
    stop: () => {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
      unlisten?.();
      unlisten = null;
    },
  };
}
