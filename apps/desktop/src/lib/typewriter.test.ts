import { describe, expect, it } from 'vitest';

import { charactersVisible } from './typewriter';

describe('how much of a line is visible', () => {
  it('shows nothing at the moment it starts', () => {
    expect(charactersVisible(0, 40)).toBe(0);
  });

  it('reveals at the configured rate', () => {
    expect(charactersVisible(1000, 400, 90)).toBe(90);
    expect(charactersVisible(2000, 400, 90)).toBe(180);
  });

  /** The whole point of deriving from elapsed time: a frame that arrives late
   *  catches up rather than leaving the line permanently behind. */
  it('catches up after a dropped frame instead of drifting', () => {
    // A gap between 1s and 3s must land where 3s says, not where three
    // increments of a counter would have.
    expect(charactersVisible(3000, 1000, 90)).toBe(270);
  });

  it('never exceeds the length of the line', () => {
    expect(charactersVisible(60_000, 12)).toBe(12);
  });

  /** A rate of zero would divide the animation into an infinite reveal; the
   *  line is shown whole instead of never finishing. */
  it('shows the whole line when the rate is not positive', () => {
    expect(charactersVisible(10, 25, 0)).toBe(25);
    expect(charactersVisible(10, 25, -5)).toBe(25);
  });

  it.each([NaN, -1, -Infinity])('shows nothing for an elapsed time of %p', (elapsed) => {
    expect(charactersVisible(elapsed, 30)).toBe(0);
  });

  it('handles an empty line without going negative', () => {
    expect(charactersVisible(500, 0)).toBe(0);
  });
});
