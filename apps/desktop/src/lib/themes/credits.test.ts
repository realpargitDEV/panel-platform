import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { THEMES } from './index';

/**
 * The credits file has to keep up with the catalogue.
 *
 * Attribution that is correct on the day it is written and silently wrong six
 * themes later is worse than none: it reads as diligence while being false. A
 * palette borrowed without its author appearing here fails the build.
 */
const CREDITS = fileURLToPath(new URL('../../../../../THIRD-PARTY-THEMES.md', import.meta.url));

describe('third-party attribution', () => {
  const text = readFileSync(CREDITS, 'utf8');

  const credited = THEMES.filter((theme) => theme.credit);

  /** Eight themes across seven works — Solarized supplies two. The file states
   *  those numbers, so a ninth borrowed palette has to update both. */
  it('credits every palette known to be borrowed', () => {
    expect(credited).toHaveLength(8);
    expect(text).toContain('Eight of the eighty-one themes');
  });

  it.each(credited.map((theme) => [theme.name, theme] as const))(
    'names %s, its author and its licence',
    (_name, theme) => {
      const credit = theme.credit;
      if (!credit) throw new Error('filtered above');

      expect(text, `${theme.id}: work missing`).toContain(credit.work);
      expect(text, `${theme.id}: author missing`).toContain(credit.author);
      expect(text, `${theme.id}: url missing`).toContain(credit.url);
    },
  );

  /** The renamed ones are the other half of the same promise: the file says
   *  what each theme was requested as and what it ships as. */
  it.each([
    'Editor Dark',
    'Repo Dark',
    'System 95',
    'System XP',
    'Classic Desktop',
    'Blockcraft',
    'Ashen',
  ])('records that %s is a rename', (name) => {
    expect(text).toContain(name);
  });

  it('does not claim a licence for a theme that has none', () => {
    for (const theme of THEMES) {
      if (theme.credit) continue;
      // An uncredited theme must not appear in the attribution table's rows.
      expect(text).not.toContain(`| ${theme.name} | ${theme.name} |`);
    }
  });
});
