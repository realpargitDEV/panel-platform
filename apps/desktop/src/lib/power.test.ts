import { describe, expect, it } from 'vitest';

import type { PowerStatus } from '../api';
import { batteryPhrase, powerLook, temperaturePhrase } from './power';

function status(overrides: Partial<PowerStatus> = {}): PowerStatus {
  return {
    mode: 'automatic',
    profile: 'balanced',
    reason: 'Processor use has averaged 4% with 1 project running.',
    preventSleep: false,
    sleepHeld: false,
    measured: true,
    cpuPercent: 4,
    memoryUsedBytes: 4_000_000_000,
    memoryTotalBytes: 16_000_000_000,
    hottestCelsius: null,
    hottestSensor: null,
    batteryPercent: null,
    charging: null,
    powerSource: 'ac',
    activeProjects: 1,
    warnings: [],
    ...overrides,
  };
}

describe('powerLook', () => {
  it('says it is measuring rather than drawing a reading it does not have', () => {
    expect(powerLook(null).label).toBe('Measuring…');
    expect(powerLook(status({ measured: false })).label).toBe('Measuring…');
  });

  it('reports the profile when there is nothing else to say', () => {
    const look = powerLook(status());
    expect(look.label).toBe('Balanced');
    expect(look.summary).toBe(status().reason);
    expect(look.tone).toBe('idle');
  });

  // A green dot that is always green stops being read.
  it('does not call an ordinary machine ok', () => {
    expect(powerLook(status()).tone).toBe('idle');
  });

  it('puts a warning ahead of the profile', () => {
    const look = powerLook(
      status({
        warnings: [{ kind: 'thermal', message: 'A sensor is reading 95°C.' }],
      }),
    );
    expect(look.tone).toBe('warn');
    expect(look.summary).toBe('A sensor is reading 95°C.');
  });

  it('puts a warning ahead of a sleep hold', () => {
    const look = powerLook(
      status({
        sleepHeld: true,
        preventSleep: true,
        warnings: [{ kind: 'low_battery', message: 'The battery is at 12%.' }],
      }),
    );
    expect(look.label).toBe('Battery low');
  });

  it('says when sleep is being held', () => {
    const look = powerLook(status({ preventSleep: true, sleepHeld: true }));
    expect(look.label).toBe('Staying awake');
    expect(look.tone).toBe('ok');
  });

  /**
   * The case the two separate fields exist for. A user who ticked "keep awake"
   * and whose machine will sleep anyway has to be told.
   */
  it('says so when the hold was asked for and refused', () => {
    const look = powerLook(status({ preventSleep: true, sleepHeld: false }));
    expect(look.tone).toBe('warn');
    expect(look.summary).toContain('may still sleep');
  });
});

describe('batteryPhrase', () => {
  it('is absent on a machine with no battery', () => {
    expect(batteryPhrase(status())).toBeNull();
  });

  it('distinguishes charging from plugged in from running down', () => {
    expect(batteryPhrase(status({ batteryPercent: 80, charging: true }))).toBe('80%, charging');
    expect(batteryPhrase(status({ batteryPercent: 80, charging: false, powerSource: 'ac' }))).toBe(
      '80%, plugged in',
    );
    expect(
      batteryPhrase(status({ batteryPercent: 80, charging: false, powerSource: 'battery' })),
    ).toBe('80%, on battery');
  });
});

describe('temperaturePhrase', () => {
  // Most Windows desktops have no readable sensor, and 0°C is not a reading.
  it('is absent rather than zero when nothing can be read', () => {
    expect(temperaturePhrase(status())).toBeNull();
  });

  it('names the sensor when there is one', () => {
    expect(temperaturePhrase(status({ hottestCelsius: 61.4, hottestSensor: 'CPU' }))).toBe(
      '61°C (CPU)',
    );
    expect(temperaturePhrase(status({ hottestCelsius: 61.4 }))).toBe('61°C');
  });
});
