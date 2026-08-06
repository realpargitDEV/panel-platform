/**
 * How the application looks, as data.
 *
 * Themes are not stylesheets here — they are a theme *id* written onto the root
 * element, which the token blocks in `themes.generated.css` respond to. Keeping
 * the choice as data and the colours as CSS means a theme switch is one
 * attribute write rather than a re-render, and nothing in the component tree
 * has to know which theme is on. That is what makes eighty-one of them cost the
 * same as five.
 *
 * The exception, deliberately, is Discord. Its surface is Discord's identity,
 * not this application's, and it opts out of every theme — see the
 * `.discord-scope` block in `styles.css`.
 */

import { DEFAULT_THEME_ID, THEME_BY_ID, resolveThemeId } from './themes';
import type { EffectId } from './themes/types';

export type ThemeId = string;

/**
 * `auto` means "whatever the theme was designed with", and is the default.
 *
 * The six named accents remain, and still apply over any theme, but they are no
 * longer what a fresh install gets: a blue accent forced onto Matrix Rain or
 * Old Newspaper stops it being that theme, and there are now eighty-one themes
 * with an accent chosen for each of them.
 */
export type AccentId = 'auto' | 'blue' | 'violet' | 'emerald' | 'amber' | 'rose' | 'cyan';
export type Density = 'comfortable' | 'compact';
/** `full` animates everything; `reduced` keeps only what conveys state; `off`
 *  removes motion entirely. The OS setting still wins over `full`. */
export type MotionLevel = 'full' | 'reduced' | 'off';

export interface Appearance {
  theme: ThemeId;
  accent: AccentId;
  density: Density;
  /** Percent of the base 14px, 90–120. */
  fontScale: number;
  motion: MotionLevel;
}

export const defaultAppearance: Appearance = {
  theme: DEFAULT_THEME_ID,
  accent: 'auto',
  density: 'comfortable',
  fontScale: 100,
  motion: 'full',
};

export const ACCENTS: { id: AccentId; label: string; value: string | null }[] = [
  // `null` renders as the theme's own accent rather than a fixed swatch.
  { id: 'auto', label: 'Theme default', value: null },
  { id: 'blue', label: 'Blue', value: '#3b82f6' },
  { id: 'violet', label: 'Violet', value: '#8b5cf6' },
  { id: 'emerald', label: 'Emerald', value: '#10b981' },
  { id: 'amber', label: 'Amber', value: '#f59e0b' },
  { id: 'rose', label: 'Rose', value: '#f43f5e' },
  { id: 'cyan', label: 'Cyan', value: '#06b6d4' },
];

const ACCENT_IDS = new Set<string>(ACCENTS.map((accent) => accent.id));

/**
 * Coerce stored or unknown input into a usable appearance.
 *
 * Everything is validated rather than trusted: this comes back from
 * `localStorage`, where a value can be anything a previous version wrote, a
 * user edited by hand, or a half-finished migration left behind. An
 * unrecognised theme must fall back, never be written to the DOM.
 *
 * The theme is *resolved* rather than merely checked, because two of the five
 * original ids were renamed when the catalogue arrived. Someone who chose
 * `dark` gets `pure-dark` — the same colours under a name that still means
 * something now that there are thirty dark themes.
 */
export function normaliseAppearance(input: unknown): Appearance {
  if (input === null || typeof input !== 'object') return defaultAppearance;
  const stored = input as Partial<Record<keyof Appearance, unknown>>;

  return {
    theme: resolveThemeId(stored.theme),
    accent:
      typeof stored.accent === 'string' && ACCENT_IDS.has(stored.accent)
        ? (stored.accent as AccentId)
        : defaultAppearance.accent,
    density:
      stored.density === 'compact' || stored.density === 'comfortable'
        ? stored.density
        : defaultAppearance.density,
    fontScale: clampFontScale(stored.fontScale),
    motion:
      stored.motion === 'full' || stored.motion === 'reduced' || stored.motion === 'off'
        ? stored.motion
        : defaultAppearance.motion,
  };
}

/** 90–120% of 14px. Below 90 the 11px labels stop being legible; above 120 the
 *  fixed-height controls the interface is built from start clipping. */
export function clampFontScale(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return defaultAppearance.fontScale;
  return Math.min(120, Math.max(90, Math.round(value)));
}

/**
 * Write an appearance onto the document.
 *
 * The only impure function here, and the only place that touches the DOM, so
 * everything above it can be tested as arithmetic.
 *
 * `auto` removes the accent attribute rather than writing a value, because the
 * accent blocks in `styles.css` exist precisely to beat the theme's own accent.
 * Writing `data-accent="auto"` would match no block, but leaving a previous
 * accent on the element would silently keep overriding every theme chosen
 * afterwards.
 */
export function applyAppearance(root: HTMLElement, appearance: Appearance): void {
  root.dataset.theme = appearance.theme;

  if (appearance.accent === 'auto') {
    delete root.dataset.accent;
  } else {
    root.dataset.accent = appearance.accent;
  }

  root.dataset.density = appearance.density;
  root.dataset.motion = appearance.motion;
  root.style.setProperty('--font-scale', `${appearance.fontScale}%`);
}

/** The background effect the current theme asks for, if any. */
export function effectFor(appearance: Appearance): EffectId | undefined {
  return THEME_BY_ID.get(appearance.theme)?.effect;
}
