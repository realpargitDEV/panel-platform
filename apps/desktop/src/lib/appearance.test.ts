import { describe, expect, it } from 'vitest';

import {
  ACCENTS,
  clampFontScale,
  defaultAppearance,
  normaliseAppearance,
  THEMES,
} from './appearance';

// `applyAppearance` needs a document and so lives in `appearance.dom.test.tsx`;
// only `*.dom.test.tsx` runs under jsdom here.

describe('themes', () => {
  it('gives every theme a distinct id', () => {
    const ids = THEMES.map((theme) => theme.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  /** The gallery renders a swatch per theme; a missing colour would render as
   *  a transparent hole rather than an obviously wrong card. */
  it('gives every theme three swatch colours', () => {
    for (const theme of THEMES) {
      expect(theme.swatch).toHaveLength(3);
      for (const colour of theme.swatch) expect(colour).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('gives every accent a distinct id and a colour', () => {
    const ids = ACCENTS.map((accent) => accent.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const accent of ACCENTS) expect(accent.value).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe('reading a stored appearance', () => {
  it('accepts a complete, valid value unchanged', () => {
    const stored = {
      theme: 'amber',
      accent: 'rose',
      density: 'compact',
      fontScale: 110,
      motion: 'reduced',
    };
    expect(normaliseAppearance(stored)).toEqual(stored);
  });

  /** A theme id this build does not know would be written to the DOM and match
   *  no token block, leaving the application unstyled. */
  it('falls back when a theme is not one this build knows', () => {
    expect(normaliseAppearance({ theme: 'solarized' }).theme).toBe(defaultAppearance.theme);
  });

  it('falls back on an unknown accent', () => {
    expect(normaliseAppearance({ accent: 'chartreuse' }).accent).toBe(defaultAppearance.accent);
  });

  it.each([null, undefined, 42, 'dark', []])(
    'survives %p where an object was expected',
    (input) => {
      expect(normaliseAppearance(input)).toEqual(defaultAppearance);
    },
  );

  it('keeps the valid half of a partly corrupt value', () => {
    const result = normaliseAppearance({ theme: 'nord', accent: 999, motion: 'sideways' });
    expect(result.theme).toBe('nord');
    expect(result.accent).toBe(defaultAppearance.accent);
    expect(result.motion).toBe(defaultAppearance.motion);
  });
});

describe('font scale', () => {
  /** Below 90 the 11px labels stop being legible; above 120 the fixed-height
   *  controls clip. Both ends are clamped rather than rejected. */
  it('clamps to the range the interface can actually render', () => {
    expect(clampFontScale(50)).toBe(90);
    expect(clampFontScale(400)).toBe(120);
    expect(clampFontScale(105)).toBe(105);
  });

  it('rounds fractional values', () => {
    expect(clampFontScale(102.4)).toBe(102);
  });

  it.each([NaN, Infinity, '110', null])('falls back on %p', (input) => {
    expect(clampFontScale(input)).toBe(defaultAppearance.fontScale);
  });
});
