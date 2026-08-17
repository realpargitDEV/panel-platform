/**
 * Turning a power status into the words and colour an interface shows.
 *
 * Here rather than in a component for the reason `projects.ts` gives: the same
 * decision is made in three places — the sidebar footer, the overview card and
 * the settings panel — and it is worth testing without rendering anything.
 */

import type { PowerMode, PowerProfile, PowerStatus } from '../api';

/** What a footer or badge shows. */
export interface PowerLook {
  /** Two or three words. Fits a collapsed sidebar. */
  label: string;
  /** The full sentence, for a tooltip or a card body. */
  summary: string;
  /**
   * `warn` when something wants saying, `ok` when the machine is being held
   * available on purpose, `idle` when there is nothing to report.
   *
   * Idle is deliberately not `ok`: a green dot that is always green stops
   * being read, and most of the time the honest answer is "nothing is
   * happening" rather than "everything is fine".
   */
  tone: 'ok' | 'warn' | 'idle';
}

const PROFILE_WORDS: Record<PowerProfile, string> = {
  performance: 'Performance',
  balanced: 'Balanced',
  efficiency: 'Efficiency',
};

const MODE_WORDS: Record<PowerMode, string> = {
  automatic: 'Automatic',
  performance: 'Performance',
  balanced: 'Balanced',
  efficiency: 'Efficiency',
  manual: 'Manual',
};

export function profileWord(profile: PowerProfile): string {
  return PROFILE_WORDS[profile] ?? 'Balanced';
}

export function modeWord(mode: PowerMode): string {
  return MODE_WORDS[mode] ?? 'Automatic';
}

/**
 * What to show for a power status.
 *
 * The order of the branches is the priority order: a warning outranks a sleep
 * hold, which outranks the profile. A user who is being told their machine is
 * at 95°C does not also need to be told it is on the balanced profile.
 */
export function powerLook(status: PowerStatus | null): PowerLook {
  if (status === null || !status.measured) {
    return {
      label: 'Measuring…',
      summary: 'The machine has not been measured yet.',
      tone: 'idle',
    };
  }

  const first = status.warnings[0];
  if (first !== undefined) {
    return {
      label:
        first.kind === 'thermal'
          ? 'Running hot'
          : first.kind === 'low_battery'
            ? 'Battery low'
            : 'Memory low',
      summary: first.message,
      tone: 'warn',
    };
  }

  if (status.sleepHeld) {
    return {
      label: 'Staying awake',
      summary: `Sleep is being held off because a project asked for it. ${status.reason}`,
      tone: 'ok',
    };
  }

  // Asked for and not granted. Worth saying: the user set the option and the
  // machine may still sleep, which they would otherwise find out the hard way.
  if (status.preventSleep && !status.sleepHeld) {
    return {
      label: 'Sleep not held',
      summary:
        'A project asked to keep this machine awake, but the system would not allow it. ' +
        'The machine may still sleep.',
      tone: 'warn',
    };
  }

  return {
    label: profileWord(status.profile),
    summary: status.reason,
    tone: 'idle',
  };
}

/** Battery, phrased for a person. `null` when there is no battery to describe. */
export function batteryPhrase(status: PowerStatus | null): string | null {
  if (status === null || status.batteryPercent === null) {
    return null;
  }
  const percent = Math.round(status.batteryPercent);
  if (status.charging === true) {
    return `${percent}%, charging`;
  }
  if (status.powerSource === 'ac') {
    return `${percent}%, plugged in`;
  }
  return `${percent}%, on battery`;
}

/**
 * Temperature, or `null` where there is nothing readable.
 *
 * Most Windows desktops expose no CPU temperature at all, and a panel showing
 * `0°C` would be inventing a reading rather than reporting one.
 */
export function temperaturePhrase(status: PowerStatus | null): string | null {
  if (status === null || status.hottestCelsius === null) {
    return null;
  }
  const rounded = Math.round(status.hottestCelsius);
  return status.hottestSensor === null ? `${rounded}°C` : `${rounded}°C (${status.hottestSensor})`;
}
