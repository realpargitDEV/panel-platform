import { describe, expect, it } from 'vitest';

import { createPainter, type DrawTarget, type EffectColours } from './effects';
import { CANVAS_EFFECTS } from './types';

const COLOURS: EffectColours = { accent: '#22ff88', canvas: '#000603', ink: '#d7ffe4' };

/** A deterministic stand-in for `Math.random`, so the same test draws the same
 *  picture on every run. */
function sequence(seed = 1): () => number {
  let state = seed;
  return () => {
    state = (state * 1103515245 + 12345) % 2147483648;
    return state / 2147483648;
  };
}

interface Drawn {
  x: number;
  y: number;
}

/** Records what a painter asked for, without a canvas. */
function recorder(): DrawTarget & { points: Drawn[]; fills: number; strokes: number } {
  const points: Drawn[] = [];
  return {
    fillStyle: '',
    strokeStyle: '',
    globalAlpha: 1,
    lineWidth: 1,
    font: '',
    points,
    fills: 0,
    strokes: 0,
    fillRect() {},
    fillText(_text, x, y) {
      points.push({ x, y });
    },
    beginPath() {},
    moveTo(x, y) {
      points.push({ x, y });
    },
    lineTo() {},
    arc(x, y) {
      points.push({ x, y });
    },
    ellipse(x, y) {
      points.push({ x, y });
    },
    fill() {
      this.fills += 1;
    },
    stroke() {
      this.strokes += 1;
    },
  };
}

describe.each(CANVAS_EFFECTS)('the %s painter', (effect) => {
  it('creates something to draw once it has a size', () => {
    const painter = createPainter(effect, COLOURS, sequence());
    expect(painter.count).toBe(0);

    painter.resize(1280, 800);
    expect(painter.count).toBeGreaterThan(0);
  });

  it('draws on every frame', () => {
    const painter = createPainter(effect, COLOURS, sequence());
    painter.resize(1280, 800);

    const target = recorder();
    painter.frame(target, 16);

    expect(target.points.length).toBeGreaterThan(0);
  });

  /**
   * Nothing may escape and stay escaped.
   *
   * Every painter either bounces, wraps or respawns at an edge. A particle that
   * drifts out of the canvas and never returns is a slow leak of the effect
   * itself: the background empties out over an afternoon, which is exactly the
   * kind of bug nobody reports and everybody notices.
   */
  it('keeps what it draws near the canvas after a long run', () => {
    const painter = createPainter(effect, COLOURS, sequence(7));
    const width = 1000;
    const height = 700;
    painter.resize(width, height);

    const target = recorder();
    for (let i = 0; i < 600; i += 1) painter.frame(target, 16);

    // Scaled to the canvas rather than a flat number of pixels. Respawn points
    // are legitimately off-screen — rain restarts a column up to half a screen
    // above the top so every column does not reappear on the same line — and a
    // fixed margin would fail that correct behaviour. Runaway drift is still
    // caught easily: 9.6 seconds of the fastest painter travels several
    // thousand pixels, an order of magnitude past this bound.
    const margin = Math.max(width, height) * 0.6;
    const recent = target.points.slice(-500);
    for (const point of recent) {
      expect(point.x, `${effect} x drifted to ${point.x}`).toBeGreaterThan(-margin);
      expect(point.x, `${effect} x drifted to ${point.x}`).toBeLessThan(width + margin);
      expect(point.y, `${effect} y drifted to ${point.y}`).toBeGreaterThan(-margin);
      expect(point.y, `${effect} y drifted to ${point.y}`).toBeLessThan(height + margin);
    }
  });

  /** A window twice the size should not cost twice the particles for ever. */
  it('caps its cost on a very large window', () => {
    const painter = createPainter(effect, COLOURS, sequence());

    painter.resize(1280, 800);
    const modest = painter.count;

    painter.resize(3840, 2160);
    const huge = painter.count;

    expect(huge).toBeLessThanOrEqual(Math.max(modest * 4, 500));
  });

  /** Resizing replaces the field rather than adding to it. */
  it('does not accumulate on repeated resizes', () => {
    const painter = createPainter(effect, COLOURS, sequence());

    painter.resize(1280, 800);
    const first = painter.count;
    for (let i = 0; i < 5; i += 1) painter.resize(1280, 800);

    expect(painter.count).toBe(first);
  });

  /** Frames are not assumed to be 16ms: a background tab that wakes up hands
   *  over whatever elapsed, and the painter must not produce NaN from it. */
  it('survives an unusual frame time', () => {
    const painter = createPainter(effect, COLOURS, sequence());
    painter.resize(800, 600);

    const target = recorder();
    painter.frame(target, 0);
    painter.frame(target, 500);

    for (const point of target.points) {
      expect(Number.isFinite(point.x)).toBe(true);
      expect(Number.isFinite(point.y)).toBe(true);
    }
  });
});

describe('the rain painter specifically', () => {
  it('fills the width in columns', () => {
    const painter = createPainter('rain', COLOURS, sequence());

    painter.resize(1600, 900);
    expect(painter.count).toBe(100);

    painter.resize(800, 900);
    expect(painter.count).toBe(50);
  });
});

describe('the particle painter specifically', () => {
  it('draws links between points that are close together', () => {
    const painter = createPainter('particles', COLOURS, sequence(3));
    painter.resize(600, 400);

    const target = recorder();
    painter.frame(target, 16);

    // A 600×400 field of ~10 points at a 130px link distance will always have
    // some pair within range; zero strokes would mean the distance test is
    // inverted.
    expect(target.strokes).toBeGreaterThan(0);
  });
});
