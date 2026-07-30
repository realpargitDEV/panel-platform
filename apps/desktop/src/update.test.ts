import { describe, expect, it } from 'vitest';
import { buttonLabel, canStart, failureMessage, idle, type InstallPhase } from './update';

describe('canStart', () => {
  it('allows a first attempt', () => {
    expect(canStart(idle)).toBe(true);
  });

  it('allows retrying after a failure', () => {
    expect(canStart({ state: 'failed', message: 'no' })).toBe(true);
  });

  /** Two concurrent installs would fight over the same files. */
  it('refuses to start a second install while one is running', () => {
    expect(canStart({ state: 'installing' })).toBe(false);
  });

  it('refuses once installed, because what is needed is a restart', () => {
    expect(canStart({ state: 'installed' })).toBe(false);
  });
});

describe('buttonLabel', () => {
  it('names the action, then the progress, then the next step', () => {
    const labels: Array<[InstallPhase, string]> = [
      [idle, 'Update now'],
      [{ state: 'installing' }, 'Updating…'],
      [{ state: 'installed' }, 'Restart to finish'],
      [{ state: 'failed', message: 'x' }, 'Try again'],
    ];

    for (const [phase, expected] of labels) {
      expect(buttonLabel(phase)).toBe(expected);
    }
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
