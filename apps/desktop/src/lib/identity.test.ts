import { describe, expect, it } from 'vitest';

import { hashString, identityFor } from './identity';

describe('hashing a key', () => {
  it('is stable for the same input', () => {
    expect(hashString('api')).toBe(hashString('api'));
  });

  /** The reason for FNV-1a over a character sum: `api` and `ipa` sum
   *  identically, and a list of similar slugs would come out one colour. */
  it('separates anagrams', () => {
    expect(hashString('api')).not.toBe(hashString('ipa'));
  });

  it('separates short similar names', () => {
    const hashes = new Set(['api', 'api-2', 'api-3', 'api-4'].map(hashString));
    expect(hashes.size).toBe(4);
  });

  it('stays a safe unsigned integer', () => {
    for (const key of ['', 'a', 'a-very-long-project-slug-'.repeat(20)]) {
      const hash = hashString(key);
      expect(Number.isSafeInteger(hash)).toBe(true);
      expect(hash).toBeGreaterThanOrEqual(0);
    }
  });
});

describe('a project identity', () => {
  it('is the same colour every time it is asked', () => {
    expect(identityFor('discord-bot')).toEqual(identityFor('discord-bot'));
  });

  it('gives different projects different hues', () => {
    expect(identityFor('api').hue).not.toBe(identityFor('web').hue);
  });

  it('lands on one of the 24 buckets', () => {
    for (const key of ['a', 'b', 'c', 'long-name', '']) {
      const { hue } = identityFor(key);
      expect(hue % 15).toBe(0);
      expect(hue).toBeGreaterThanOrEqual(0);
      expect(hue).toBeLessThan(360);
    }
  });

  /** Every mark must sit at the same visual weight, or a list looks unbalanced
   *  rather than varied. Saturation and lightness are therefore fixed. */
  it('varies hue only, never saturation or lightness', () => {
    const first = identityFor('one');
    const second = identityFor('two');

    expect(first.from).toMatch(/72% 58%/);
    expect(second.from).toMatch(/72% 58%/);
    expect(first.to).toMatch(/68% 44%/);
    expect(second.to).toMatch(/68% 44%/);
  });

  it('keeps the second stop inside a legal hue', () => {
    for (let bucket = 0; bucket < 24; bucket += 1) {
      const { to } = identityFor(`key-${bucket}`);
      const hue = Number(/hsl\((\d+(?:\.\d+)?)/.exec(to)?.[1]);
      expect(hue).toBeGreaterThanOrEqual(0);
      expect(hue).toBeLessThan(360);
    }
  });

  it('handles an empty key rather than producing NaN', () => {
    expect(identityFor('').from).not.toContain('NaN');
  });
});
