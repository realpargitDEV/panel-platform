import { describe, expect, it } from 'vitest';

import {
  AUTO_SCROLL_EDGE,
  AUTO_SCROLL_MAX,
  autoScrollAmount,
  combineRubberBand,
  DRAG_THRESHOLD,
  intersects,
  normaliseRect,
  passedThreshold,
  rowsInRect,
  startsRubberBand,
  type RowBox,
} from './rubberBand';

/** Five 22px rows in content coordinates, as the tree draws them. */
const rows: RowBox[] = ['a', 'b', 'c', 'd', 'e'].map((path, index) => ({
  path,
  top: index * 22,
  bottom: index * 22 + 22,
  left: 0,
  right: 240,
}));

describe('normalising the rectangle', () => {
  it('keeps a downward-right drag as it is', () => {
    expect(normaliseRect(10, 20, 100, 200)).toEqual({
      left: 10,
      top: 20,
      right: 100,
      bottom: 200,
    });
  });

  it('flips a drag that went up and to the left', () => {
    // Used as-is this is a negative box, and nothing ever intersects it.
    expect(normaliseRect(100, 200, 10, 20)).toEqual({
      left: 10,
      top: 20,
      right: 100,
      bottom: 200,
    });
  });

  it('handles a drag that went only one way', () => {
    expect(normaliseRect(50, 10, 20, 90)).toEqual({ left: 20, top: 10, right: 50, bottom: 90 });
  });
});

describe('intersection', () => {
  const box = { left: 0, top: 0, right: 100, bottom: 100 };

  it('is true for an overlap', () => {
    expect(intersects(box, { left: 50, top: 50, right: 150, bottom: 150 })).toBe(true);
  });

  it('is false for a box that is entirely outside', () => {
    expect(intersects(box, { left: 200, top: 200, right: 300, bottom: 300 })).toBe(false);
  });

  it('is false for edges that merely touch', () => {
    // A zero-height drag along a row boundary must not select both rows.
    expect(intersects(box, { left: 100, top: 0, right: 200, bottom: 100 })).toBe(false);
  });
});

describe('picking rows', () => {
  it('selects every row the box touches, even partly', () => {
    // From halfway down row b to halfway down row d.
    expect(rowsInRect(rows, { left: 0, top: 33, right: 240, bottom: 77 })).toEqual(['b', 'c', 'd']);
  });

  it('selects one row for a small box inside it', () => {
    expect(rowsInRect(rows, { left: 10, top: 26, right: 60, bottom: 30 })).toEqual(['b']);
  });

  it('selects nothing below the last row', () => {
    expect(rowsInRect(rows, { left: 0, top: 300, right: 240, bottom: 400 })).toEqual([]);
  });

  it('works the same when the list is scrolled, because coordinates are content-relative', () => {
    // Row c after scrolling 44px: the caller adds scrollTop to the pointer, so
    // the same content box still names the same row.
    const scrolled = { left: 0, top: 45, right: 240, bottom: 60 };
    expect(rowsInRect(rows, scrolled)).toEqual(['c']);
  });

  it('respects the horizontal extent too', () => {
    expect(rowsInRect(rows, { left: 400, top: 0, right: 500, bottom: 100 })).toEqual([]);
  });
});

describe('the drag threshold', () => {
  it('ignores a press that barely moved', () => {
    expect(passedThreshold(100, 100, 101, 102)).toBe(false);
  });

  it('starts once the pointer has travelled far enough', () => {
    expect(passedThreshold(100, 100, 100 + DRAG_THRESHOLD, 100)).toBe(true);
    expect(passedThreshold(100, 100, 100, 100 - DRAG_THRESHOLD)).toBe(true);
  });
});

describe('auto-scrolling at the edges', () => {
  it('does not scroll in the middle', () => {
    expect(autoScrollAmount(300, 100, 500)).toBe(0);
  });

  it('scrolls up near the top', () => {
    expect(autoScrollAmount(105, 100, 500)).toBeLessThan(0);
  });

  it('scrolls down near the bottom', () => {
    expect(autoScrollAmount(495, 100, 500)).toBeGreaterThan(0);
  });

  it('speeds up the deeper into the edge the pointer goes', () => {
    const shallow = autoScrollAmount(100 + AUTO_SCROLL_EDGE - 4, 100, 500);
    const deep = autoScrollAmount(100, 100, 500);
    expect(Math.abs(deep)).toBeGreaterThan(Math.abs(shallow));
    expect(Math.abs(deep)).toBeLessThanOrEqual(AUTO_SCROLL_MAX);
  });

  it('returns nothing for a viewport with no height', () => {
    expect(autoScrollAmount(100, 200, 200)).toBe(0);
  });
});

describe('deciding whether a press starts a rectangle', () => {
  function target(matches: string[]) {
    return { closest: (selector: string) => (matches.includes(selector) ? {} : null) };
  }
  const ROW = '[data-row], button, input, [role="treeitem"]';

  it('starts on empty space', () => {
    expect(startsRubberBand(target([]))).toBe(true);
  });

  it('does not start on a row, which would make files undraggable', () => {
    expect(startsRubberBand(target([ROW]))).toBe(false);
  });

  it('never throws on something that cannot be asked', () => {
    expect(startsRubberBand(null)).toBe(false);
    expect(startsRubberBand(globalThis)).toBe(false);
  });
});

describe('combining with the previous selection', () => {
  it('replaces it by default', () => {
    expect(combineRubberBand(['a', 'b'], ['c'], false)).toEqual(['c']);
  });

  it('adds to it when ctrl is held', () => {
    expect(combineRubberBand(['a'], ['b', 'c'], true)).toEqual(['a', 'b', 'c']);
  });

  it('does not duplicate a row that was already selected', () => {
    expect(combineRubberBand(['a', 'b'], ['b', 'c'], true)).toEqual(['a', 'b', 'c']);
  });

  it('measures against the snapshot, so sweeping back does not toggle rows', () => {
    // The box grows to cover b then shrinks off it again; b should simply be
    // absent, not toggled twice into a different state.
    const before = ['a'];
    expect(combineRubberBand(before, ['b'], true)).toEqual(['a', 'b']);
    expect(combineRubberBand(before, [], true)).toEqual(['a']);
  });
});
