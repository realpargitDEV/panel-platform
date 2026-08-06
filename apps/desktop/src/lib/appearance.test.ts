import { describe, expect, it } from 'vitest';

import {
  ACCENTS,
  clampFontScale,
  defaultAppearance,
  effectFor,
  normaliseAppearance,
} from './appearance';
import { THEMES } from './themes';
import type { EffectId } from './themes/types';

// `applyAppearance` needs a document and so lives in `appearance.dom.test.tsx`;
// only `*.dom.test.tsx` runs under jsdom here.

describe('accents', () => {
  it('gives every accent a distinct id', () => {
    const ids = ACCENTS.map((accent) => accent.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('gives every accent but Auto a colour to render', () => {
    for (const accent of ACCENTS) {
      if (accent.id === 'auto') {
        expect(accent.value).toBeNull();
      } else {
        expect(accent.value).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  /** Auto is the default so that a theme arrives looking the way it was
   *  designed, rather than with a blue accent painted over it. */
  it('defaults to the theme’s own accent', () => {
    expect(defaultAppearance.accent).toBe('auto');
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
    expect(normaliseAppearance({ theme: 'not-a-theme' }).theme).toBe(defaultAppearance.theme);
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

/**
 * The upgrade path.
 *
 * Five ids were in the wild before the catalogue existed. Anyone carrying one
 * of them must open the application to the theme they chose, not to the
 * default — a settings screen that silently resets is indistinguishable from
 * one that lost the setting.
 */
describe('migrating an appearance stored by an earlier version', () => {
  it.each([
    ['dark', 'pure-dark'],
    ['light', 'pure-light'],
    ['amber', 'amber'],
    ['midnight', 'midnight'],
    ['nord', 'nord'],
  ])('turns the stored %s into %s', (stored, expected) => {
    expect(normaliseAppearance({ theme: stored }).theme).toBe(expected);
  });

  /** Everything else in an old record is still in the shape this version
   *  expects, and must survive the theme being rewritten. */
  it('keeps the rest of an old record intact', () => {
    const result = normaliseAppearance({
      theme: 'light',
      accent: 'cyan',
      density: 'compact',
      fontScale: 115,
      motion: 'off',
    });

    expect(result).toEqual({
      theme: 'pure-light',
      accent: 'cyan',
      density: 'compact',
      fontScale: 115,
      motion: 'off',
    });
  });
});

describe('the effect a theme asks for', () => {
  it('reports nothing for a theme with no effect', () => {
    expect(effectFor({ ...defaultAppearance, theme: 'pure-dark' })).toBeUndefined();
  });

  it('reports the effect for a theme that has one', () => {
    expect(effectFor({ ...defaultAppearance, theme: 'matrix-rain' })).toBe('rain');
  });

  it('reports nothing rather than throwing for an id that is not a theme', () => {
    expect(effectFor({ ...defaultAppearance, theme: 'nonsense' })).toBeUndefined();
  });

  it('only ever names an effect some theme actually declares', () => {
    const declared = new Set(
      THEMES.map((theme) => theme.effect).filter((effect): effect is EffectId => Boolean(effect)),
    );
    for (const theme of THEMES) {
      const effect = effectFor({ ...defaultAppearance, theme: theme.id });
      if (effect) expect(declared.has(effect)).toBe(true);
    }
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
