import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { renderAllThemes } from './css';
import { THEMES } from './index';

/**
 * The stylesheet is generated from the catalogue, and this is what proves the
 * two have not drifted apart.
 *
 * Generation deliberately runs here rather than from a standalone script: the
 * catalogue is TypeScript, so any script would need its own loader, and this
 * file already has one. `pnpm test:themes:update` re-runs it with the write
 * enabled; every other run only compares, so a hand-edit to the generated file
 * fails CI instead of quietly surviving until the next regeneration.
 */
const GENERATED = fileURLToPath(new URL('../../themes.generated.css', import.meta.url));

describe('the generated stylesheet', () => {
  const expected = renderAllThemes(THEMES);

  if (process.env.UPDATE_THEME_CSS) {
    it('is rewritten from the catalogue', () => {
      writeFileSync(GENERATED, expected, 'utf8');
      expect(readFileSync(GENERATED, 'utf8')).toBe(expected);
    });
    return;
  }

  it('matches the catalogue it was generated from', () => {
    let actual: string;
    try {
      actual = readFileSync(GENERATED, 'utf8');
    } catch {
      throw new Error(`${GENERATED} is missing. Run: pnpm test:themes:update`);
    }

    if (actual !== expected) {
      throw new Error(
        'themes.generated.css is out of date with the theme catalogue.\n' +
          'Run: pnpm test:themes:update',
      );
    }

    expect(actual).toBe(expected);
  });

  it('emits a block for every theme', () => {
    for (const theme of THEMES) {
      expect(expected, theme.id).toContain(`:root[data-theme='${theme.id}']`);
    }
  });

  /** The alias names are a promised public surface for custom CSS, so their
   *  absence is a break rather than a cosmetic change. */
  it('emits the alias vocabulary beside the canonical tokens', () => {
    for (const alias of [
      '--background',
      '--surface',
      '--primary',
      '--text',
      '--text-muted',
      '--border',
      '--success',
      '--warning',
      '--danger',
      '--border-radius',
      '--font-family',
    ]) {
      expect(expected, alias).toContain(`${alias}:`);
    }
  });
});
