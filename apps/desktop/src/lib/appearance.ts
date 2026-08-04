/**
 * How the application looks, as data.
 *
 * Themes are not stylesheets here — they are a theme *id* written onto the
 * root element, which the token blocks in `styles.css` respond to. Keeping the
 * choice as data and the colours as CSS means a theme switch is one attribute
 * write rather than a re-render, and nothing in the component tree has to know
 * which theme is on.
 *
 * The exception, deliberately, is Discord. Its surface is Discord's identity,
 * not this application's, and it opts out of every theme — see the
 * `.discord-scope` block in `styles.css`.
 */

export type ThemeId = 'dark' | 'light' | 'amber' | 'midnight' | 'nord';
export type AccentId = 'blue' | 'violet' | 'emerald' | 'amber' | 'rose' | 'cyan';
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
  theme: 'dark',
  accent: 'blue',
  density: 'comfortable',
  fontScale: 100,
  motion: 'full',
};

/** The swatches are canvas / raised / accent, in that order — enough to show
 *  what a theme does without rendering the whole application into a card. */
export const THEMES: {
  id: ThemeId;
  label: string;
  detail: string;
  swatch: [string, string, string];
}[] = [
  {
    id: 'dark',
    label: 'Dark',
    detail: 'The default charcoal. Neutral, so project colours stay readable.',
    swatch: ['#0e0e10', '#1c1c21', '#3b82f6'],
  },
  {
    id: 'light',
    label: 'Light',
    detail: 'For bright rooms and screen sharing.',
    swatch: ['#f7f7f9', '#ffffff', '#2563eb'],
  },
  {
    id: 'amber',
    label: 'Amber',
    detail: 'Warm greys with a low-blue cast, for long evenings.',
    swatch: ['#14100b', '#241d14', '#f59e0b'],
  },
  {
    id: 'midnight',
    label: 'Midnight',
    detail: 'Deep blue-black with more contrast between layers.',
    swatch: ['#0a0d16', '#161b2b', '#6366f1'],
  },
  {
    id: 'nord',
    label: 'Nord',
    detail: 'Cool slate, softer whites, lower overall contrast.',
    swatch: ['#161a21', '#232a35', '#88c0d0'],
  },
];

export const ACCENTS: { id: AccentId; label: string; value: string }[] = [
  { id: 'blue', label: 'Blue', value: '#3b82f6' },
  { id: 'violet', label: 'Violet', value: '#8b5cf6' },
  { id: 'emerald', label: 'Emerald', value: '#10b981' },
  { id: 'amber', label: 'Amber', value: '#f59e0b' },
  { id: 'rose', label: 'Rose', value: '#f43f5e' },
  { id: 'cyan', label: 'Cyan', value: '#06b6d4' },
];

const THEME_IDS = new Set<string>(THEMES.map((theme) => theme.id));
const ACCENT_IDS = new Set<string>(ACCENTS.map((accent) => accent.id));

/**
 * Coerce stored or unknown input into a usable appearance.
 *
 * Everything is validated rather than trusted: this comes back from
 * `localStorage`, where a value can be anything a previous version wrote, a
 * user edited by hand, or a half-finished migration left behind. An
 * unrecognised theme must fall back, never be written to the DOM.
 */
export function normaliseAppearance(input: unknown): Appearance {
  if (input === null || typeof input !== 'object') return defaultAppearance;
  const stored = input as Partial<Record<keyof Appearance, unknown>>;

  return {
    theme:
      typeof stored.theme === 'string' && THEME_IDS.has(stored.theme)
        ? (stored.theme as ThemeId)
        : defaultAppearance.theme,
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
 */
export function applyAppearance(root: HTMLElement, appearance: Appearance): void {
  root.dataset.theme = appearance.theme;
  root.dataset.accent = appearance.accent;
  root.dataset.density = appearance.density;
  root.dataset.motion = appearance.motion;
  root.style.setProperty('--font-scale', `${appearance.fontScale}%`);
}
