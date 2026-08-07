import { describe, expect, it } from 'vitest';

import {
  contrast,
  contrastRounded,
  darken,
  darkenUntilWhiteReadable,
  isDark,
  lighten,
  mix,
  nudgeUntilReadable,
  parseHex,
  relativeLuminance,
  toHex,
} from './color';

describe('parsing hex', () => {
  it('reads the long form', () => {
    expect(parseHex('#3b82f6')).toEqual({ r: 0x3b, g: 0x82, b: 0xf6 });
  });

  it('expands the short form', () => {
    expect(parseHex('#fff')).toEqual({ r: 255, g: 255, b: 255 });
    expect(parseHex('#08f')).toEqual({ r: 0, g: 0x88, b: 0xff });
  });

  it('does not require the hash', () => {
    expect(parseHex('000000')).toEqual({ r: 0, g: 0, b: 0 });
  });

  /** A typo in the catalogue must stop the build, not ship as black: a theme
   *  whose canvas silently became #000 would be shipped by whoever wrote it. */
  it('throws on anything that is not a colour', () => {
    expect(() => parseHex('rebeccapurple')).toThrow(/Not a hex colour/);
    expect(() => parseHex('#12345')).toThrow(/Not a hex colour/);
    expect(() => parseHex('')).toThrow(/Not a hex colour/);
  });

  it('round-trips through toHex', () => {
    expect(toHex(parseHex('#3b82f6'))).toBe('#3b82f6');
  });

  it('clamps and pads when writing', () => {
    expect(toHex({ r: -20, g: 300, b: 7 })).toBe('#00ff07');
  });
});

describe('relative luminance', () => {
  it('anchors at black and white', () => {
    expect(relativeLuminance(parseHex('#000000'))).toBe(0);
    expect(relativeLuminance(parseHex('#ffffff'))).toBeCloseTo(1, 5);
  });

  /** The sRGB transfer is not linear. Mid grey sits near 0.216, not 0.5 —
   *  treating it as 0.5 would pass palettes that are unreadable in practice. */
  it('applies the sRGB transfer rather than a linear one', () => {
    expect(relativeLuminance(parseHex('#808080'))).toBeCloseTo(0.2159, 3);
  });
});

describe('contrast', () => {
  it('reaches 21:1 between black and white', () => {
    expect(contrastRounded('#000000', '#ffffff')).toBe(21);
  });

  it('is 1:1 for a colour against itself', () => {
    expect(contrastRounded('#3b82f6', '#3b82f6')).toBe(1);
  });

  it('does not care which argument is the background', () => {
    expect(contrast('#16161a', '#e8e8ec')).toBeCloseTo(contrast('#e8e8ec', '#16161a'), 10);
  });

  /** The measurement that decided the primary action's gradient: white 13px
   *  text on the plain accent fails AA, which is why it is never used raw. */
  it('measures the accent that failed AA at 3.68:1', () => {
    expect(contrastRounded('#3b82f6', '#ffffff')).toBe(3.68);
  });

  it('accepts parsed colours as well as strings', () => {
    expect(contrastRounded(parseHex('#000'), '#fff')).toBe(21);
  });
});

describe('mixing', () => {
  it('returns the ends untouched', () => {
    expect(mix('#3b82f6', '#000000', 0)).toBe('#3b82f6');
    expect(mix('#3b82f6', '#000000', 1)).toBe('#000000');
  });

  it('meets in the middle', () => {
    expect(mix('#000000', '#ffffff', 0.5)).toBe('#808080');
  });

  it('clamps out-of-range amounts rather than extrapolating', () => {
    expect(mix('#000000', '#ffffff', 5)).toBe('#ffffff');
    expect(mix('#000000', '#ffffff', -5)).toBe('#000000');
  });

  it('darkens and lightens in the expected directions', () => {
    expect(relativeLuminance(parseHex(darken('#3b82f6', 0.3)))).toBeLessThan(
      relativeLuminance(parseHex('#3b82f6')),
    );
    expect(relativeLuminance(parseHex(lighten('#3b82f6', 0.3)))).toBeGreaterThan(
      relativeLuminance(parseHex('#3b82f6')),
    );
  });
});

describe('isDark', () => {
  it('sorts canvases by which text weight they need', () => {
    expect(isDark('#0e0e10')).toBe(true);
    expect(isDark('#16161a')).toBe(true);
    expect(isDark('#f4f4f6')).toBe(false);
    expect(isDark('#ffffff')).toBe(false);
  });
});

describe('darkening until white text is readable', () => {
  it('clears AA for the accent that fails it raw', () => {
    const brand = darkenUntilWhiteReadable('#3b82f6');

    expect(contrast('#3b82f6', '#ffffff')).toBeLessThan(4.5);
    expect(contrast(brand, '#ffffff')).toBeGreaterThanOrEqual(4.5);
  });

  /** It must stop at the first value that clears the bar. A theme's accent is
   *  its identity, and overshooting into near-black would lose it. */
  it('stays as close to the stated accent as the target allows', () => {
    const brand = darkenUntilWhiteReadable('#3b82f6');

    expect(contrast(brand, '#ffffff')).toBeLessThan(5.5);
  });

  it('leaves a colour that already passes alone', () => {
    expect(darkenUntilWhiteReadable('#1d4ed8')).toBe('#1d4ed8');
  });

  it('honours a stricter target', () => {
    expect(contrast(darkenUntilWhiteReadable('#3b82f6', 7), '#ffffff')).toBeGreaterThanOrEqual(7);
  });
});

describe('nudging text until it is readable', () => {
  it('lightens text on a dark canvas', () => {
    const canvas = '#0e0e10';
    const quiet = '#4a4a52';

    const fixed = nudgeUntilReadable(quiet, canvas, 4.5);

    expect(contrast(quiet, canvas)).toBeLessThan(4.5);
    expect(contrast(fixed, canvas)).toBeGreaterThanOrEqual(4.5);
    expect(relativeLuminance(parseHex(fixed))).toBeGreaterThan(relativeLuminance(parseHex(quiet)));
  });

  it('darkens text on a light canvas', () => {
    const canvas = '#f4f4f6';
    const quiet = '#b4b4bc';

    const fixed = nudgeUntilReadable(quiet, canvas, 4.5);

    expect(contrast(fixed, canvas)).toBeGreaterThanOrEqual(4.5);
    expect(relativeLuminance(parseHex(fixed))).toBeLessThan(relativeLuminance(parseHex(quiet)));
  });

  it('leaves text that already reads alone', () => {
    expect(nudgeUntilReadable('#e8e8ec', '#0e0e10', 4.5)).toBe('#e8e8ec');
  });
});
