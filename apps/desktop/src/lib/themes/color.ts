/**
 * Colour arithmetic, so eighty themes do not hand-write four hundred values.
 *
 * Every theme states the handful of colours that actually carry its identity —
 * the canvas, the surfaces above it, three text weights and one accent. The
 * hover states, the tinted `-soft` backgrounds, the sidebar gradient and the
 * brand gradient are *derived* from those here. Hand-listing them would be four
 * hundred more chances to write a value that disagrees with the theme it
 * belongs to, and no reviewer would catch the one that did.
 *
 * The contrast maths is WCAG 2.1: it is what the accessibility gate measures
 * against, and it is the reason the brand gradient is computed rather than
 * chosen. `#3b82f6` under white 13px text measures 3.68:1 and fails AA — that
 * was found by measuring, not by looking, and every theme now gets the same
 * treatment automatically.
 */

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/**
 * Parse `#rgb` or `#rrggbb`.
 *
 * Throws rather than returning a fallback: a malformed colour in the catalogue
 * is a typo, and a silent black would ship as a theme nobody could read.
 */
export function parseHex(hex: string): Rgb {
  const value = hex.trim().replace(/^#/, '');

  const full =
    value.length === 3
      ? value
          .split('')
          .map((c) => c + c)
          .join('')
      : value;

  if (!/^[0-9a-fA-F]{6}$/.test(full)) {
    throw new Error(`Not a hex colour: "${hex}"`);
  }

  return {
    r: parseInt(full.slice(0, 2), 16),
    g: parseInt(full.slice(2, 4), 16),
    b: parseInt(full.slice(4, 6), 16),
  };
}

export function toHex({ r, g, b }: Rgb): string {
  const channel = (n: number) =>
    Math.max(0, Math.min(255, Math.round(n)))
      .toString(16)
      .padStart(2, '0');
  return `#${channel(r)}${channel(g)}${channel(b)}`;
}

/**
 * Relative luminance, WCAG 2.1 §relative-luminance.
 *
 * The channel transfer is not a simple divide by 255: sRGB is gamma-encoded,
 * and treating it as linear overstates the luminance of dark colours badly
 * enough to pass palettes that are genuinely unreadable.
 */
export function relativeLuminance(colour: Rgb): number {
  const channel = (raw: number) => {
    const c = raw / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };

  return 0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b);
}

/** WCAG contrast ratio, 1:1 to 21:1. Order of arguments does not matter. */
export function contrast(a: string | Rgb, b: string | Rgb): number {
  const first = typeof a === 'string' ? parseHex(a) : a;
  const second = typeof b === 'string' ? parseHex(b) : b;

  const one = relativeLuminance(first);
  const two = relativeLuminance(second);
  const lighter = Math.max(one, two);
  const darker = Math.min(one, two);

  return (lighter + 0.05) / (darker + 0.05);
}

/** Rounded to two places, which is how contrast is quoted and compared. */
export function contrastRounded(a: string | Rgb, b: string | Rgb): number {
  return Math.round(contrast(a, b) * 100) / 100;
}

/**
 * Mix two colours, `amount` being how much of `b` ends up in the result.
 *
 * Deliberately mixed in sRGB rather than a perceptual space: these values sit
 * beside `color-mix(in srgb, …)` declarations in the stylesheet, and two mixing
 * models would make a derived token and its CSS equivalent disagree.
 */
export function mix(a: string, b: string, amount: number): string {
  const from = parseHex(a);
  const to = parseHex(b);
  const t = Math.max(0, Math.min(1, amount));

  return toHex({
    r: from.r + (to.r - from.r) * t,
    g: from.g + (to.g - from.g) * t,
    b: from.b + (to.b - from.b) * t,
  });
}

export function darken(colour: string, amount: number): string {
  return mix(colour, '#000000', amount);
}

export function lighten(colour: string, amount: number): string {
  return mix(colour, '#ffffff', amount);
}

/** True where the background wants light text on it. */
export function isDark(colour: string): boolean {
  return relativeLuminance(parseHex(colour)) < 0.22;
}

/**
 * Darken a colour until white text on it clears a contrast target.
 *
 * This is what makes the primary action safe in every theme. A theme states one
 * accent — the colour it wants to be recognised by — and the button that
 * carries white 13px text is built from a darkened form of it rather than from
 * the accent itself. Stepping in 2% increments keeps the result as close to the
 * stated accent as the requirement allows, instead of jumping to a value that
 * clears the bar but no longer looks like the theme.
 *
 * Returns black in the impossible case, which cannot fail the gate and is
 * visibly wrong enough to be noticed if the loop ever changed.
 */
export function darkenUntilWhiteReadable(colour: string, target = 4.5): string {
  for (let step = 0; step <= 50; step += 1) {
    const candidate = darken(colour, step * 0.02);
    if (contrast(candidate, '#ffffff') >= target) return candidate;
  }
  return '#000000';
}

/**
 * Move a foreground colour toward or away from its background until it clears a
 * contrast target, keeping its hue.
 *
 * Used for the muted and faint text weights. A theme picks the grey it wants;
 * if that grey is a step too quiet to read against the canvas it belongs to,
 * this walks it toward the readable side rather than the theme shipping text
 * nobody can use. The direction follows the background: text on a dark canvas
 * gets lighter, text on a light canvas gets darker.
 */
export function nudgeUntilReadable(foreground: string, background: string, target: number): string {
  if (contrast(foreground, background) >= target) return foreground;

  const towards = isDark(background) ? '#ffffff' : '#000000';

  for (let step = 1; step <= 50; step += 1) {
    const candidate = mix(foreground, towards, step * 0.02);
    if (contrast(candidate, background) >= target) return candidate;
  }

  return towards;
}
