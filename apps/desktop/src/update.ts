/**
 * The update prompt's state machine, and the words it shows.
 *
 * Separated from the components because both the banner and the Settings screen
 * offer the same action and must behave identically — and because the parts
 * worth testing are these, not the markup.
 */

/** Where an install attempt has got to. */
export type InstallPhase =
  | { state: 'idle' }
  | { state: 'installing' }
  /** Only reachable on Linux. On Windows the process exits mid-install. */
  | { state: 'installed' }
  | { state: 'failed'; message: string };

export const idle: InstallPhase = { state: 'idle' };

/** An install may be started only when nothing is already running. */
export function canStart(phase: InstallPhase): boolean {
  return phase.state === 'idle' || phase.state === 'failed';
}

export function buttonLabel(phase: InstallPhase): string {
  switch (phase.state) {
    case 'installing':
      return 'Updating…';
    case 'installed':
      return 'Restart to finish';
    case 'failed':
      return 'Try again';
    default:
      return 'Update now';
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
