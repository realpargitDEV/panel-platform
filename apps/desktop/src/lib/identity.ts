/**
 * A project's visual identity, derived rather than chosen.
 *
 * A letter in a coloured box is what an application shows when it has nothing
 * to say about a project. This says two things: *what* the project is, through
 * its runtime glyph, and *which* project it is, through a colour pair that is a
 * pure function of its identifier.
 *
 * Derived, not random, and not stored: the same project is the same colour on
 * every machine, in every list, after every reinstall, with no column to
 * migrate and nothing to keep in sync. Two projects can collide on hue — with
 * 24 buckets that is expected, not a bug — which is why the glyph carries the
 * meaning and the colour only carries recognition.
 */

/**
 * FNV-1a. Chosen because it is short, has no dependencies, and spreads short
 * similar strings — `api`, `api-2`, `api-3` — into different buckets, which a
 * naive character sum does not.
 */
export function hashString(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    // The FNV prime, via shifts: a plain multiply overflows into the range
    // where JavaScript numbers stop being exact integers.
    hash = (hash + ((hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24))) >>> 0;
  }
  return hash >>> 0;
}

/** 24 buckets of 15°. Enough that neighbours in a list rarely repeat, few
 *  enough that every bucket is a hue a human would call a different colour. */
const BUCKETS = 24;

export interface Identity {
  from: string;
  to: string;
  /** The hue itself, for anything that needs to tint beside the mark. */
  hue: number;
}

/**
 * The gradient for a project.
 *
 * Saturation and lightness are fixed so that every project sits at the same
 * weight: a mark that varied in all three would give some projects a
 * washed-out badge and others a fluorescent one, and the list would look
 * unbalanced rather than varied.
 */
export function identityFor(key: string): Identity {
  const hue = (hashString(key) % BUCKETS) * (360 / BUCKETS);

  return {
    hue,
    from: `hsl(${hue} 72% 58%)`,
    // The second stop rotates forward rather than darkening, which is what
    // keeps the tile reading as lit rather than as a flat colour with a shadow.
    to: `hsl(${(hue + 26) % 360} 68% 44%)`,
  };
}
