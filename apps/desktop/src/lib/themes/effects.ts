/**
 * The canvas background effects.
 *
 * Kept out of the component on purpose. A painter here is a plain object with
 * `resize` and `frame`, holding its own particles and knowing nothing about
 * React, the document or the clock — which means the interesting parts (how
 * many particles a size deserves, whether anything escapes the canvas, whether
 * a frame draws at all) can be tested without a browser.
 *
 * Every painter takes its randomness as an argument so a test can hand it a
 * sequence and get the same picture twice. None of them keeps a timer: the
 * component owns the frame loop, because the component is the only thing that
 * knows whether the window is still being looked at.
 */

import type { CanvasEffectId } from './types';

export interface EffectColours {
  /** The theme's accent, which every effect is drawn in some form of. */
  accent: string;
  /** The canvas colour, used to fade previous frames rather than clearing. */
  canvas: string;
  ink: string;
}

/**
 * The subset of the 2D context these painters use.
 *
 * Named rather than taking `CanvasRenderingContext2D` so a test can pass a
 * recording stub without constructing a real canvas.
 */
export interface DrawTarget {
  // Widened to what a real 2D context declares, or a `CanvasRenderingContext2D`
  // would not be assignable to this interface at all. The painters only ever
  // write strings into them.
  fillStyle: string | CanvasGradient | CanvasPattern;
  strokeStyle: string | CanvasGradient | CanvasPattern;
  globalAlpha: number;
  lineWidth: number;
  font: string;
  fillRect(x: number, y: number, w: number, h: number): void;
  fillText(text: string, x: number, y: number): void;
  beginPath(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  arc(x: number, y: number, r: number, start: number, end: number): void;
  ellipse?(
    x: number,
    y: number,
    rx: number,
    ry: number,
    rotation: number,
    start: number,
    end: number,
  ): void;
  fill(): void;
  stroke(): void;
}

export interface EffectPainter {
  resize(width: number, height: number): void;
  /** Advance and draw one frame. `deltaMs` is clamped by the caller. */
  frame(target: DrawTarget, deltaMs: number): void;
  /** For tests and for reasoning about cost. */
  readonly count: number;
}

type Random = () => number;

/**
 * Particle counts are per area, not fixed.
 *
 * A fixed count is either sparse on a large window or a slideshow on a small
 * one. Density is expressed per megapixel and then capped, so a 4K window costs
 * a predictable maximum rather than four times a 1080p one.
 */
function countFor(width: number, height: number, perMegapixel: number, cap: number): number {
  const megapixels = (width * height) / 1_000_000;
  return Math.max(8, Math.min(cap, Math.round(megapixels * perMegapixel)));
}

/** Fade the previous frame toward the canvas colour instead of clearing it.
 *  This is what leaves trails behind rain and embers. */
function fade(target: DrawTarget, colour: string, alpha: number, w: number, h: number): void {
  target.globalAlpha = alpha;
  target.fillStyle = colour;
  target.fillRect(0, 0, w, h);
  target.globalAlpha = 1;
}

/* ------------------------------------------------------------------- rain */

const GLYPHS = 'アイウエオカキクケコサシスセソタチツテトナニヌネノ0123456789';

interface Column {
  x: number;
  head: number;
  speed: number;
}

function createRain(colours: EffectColours, random: Random): EffectPainter {
  const spacing = 16;
  let width = 0;
  let height = 0;
  let columns: Column[] = [];

  const glyph = () => GLYPHS[Math.floor(random() * GLYPHS.length)] ?? '0';

  return {
    get count() {
      return columns.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      const count = Math.max(1, Math.floor(w / spacing));
      columns = Array.from({ length: count }, (_, index) => ({
        x: index * spacing,
        // Staggered above the top edge, or every column starts on one line and
        // the first second of the effect is a single falling row.
        head: random() * -h,
        speed: 40 + random() * 90,
      }));
    },
    frame(target, deltaMs) {
      fade(target, colours.canvas, 0.14, width, height);
      target.font = `${spacing}px monospace`;

      for (const column of columns) {
        column.head += (column.speed * deltaMs) / 1000;
        if (column.head > height + spacing) column.head = -random() * height * 0.5;

        // The leading glyph is the bright one; the trail is what the fade above
        // leaves behind, so only two glyphs are ever drawn per column.
        target.globalAlpha = 1;
        target.fillStyle = colours.ink;
        target.fillText(glyph(), column.x, column.head);

        target.globalAlpha = 0.55;
        target.fillStyle = colours.accent;
        target.fillText(glyph(), column.x, column.head - spacing);
      }
      target.globalAlpha = 1;
    },
  };
}

/* ------------------------------------------------------------------ stars */

interface Star {
  x: number;
  y: number;
  r: number;
  phase: number;
  rate: number;
}

function createStars(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let stars: Star[] = [];

  return {
    get count() {
      return stars.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      stars = Array.from({ length: countFor(w, h, 220, 420) }, () => ({
        x: random() * w,
        y: random() * h,
        r: 0.4 + random() * 1.3,
        phase: random() * Math.PI * 2,
        rate: 0.4 + random() * 1.1,
      }));
    },
    frame(target, deltaMs) {
      target.fillStyle = colours.canvas;
      target.globalAlpha = 1;
      target.fillRect(0, 0, width, height);

      for (const star of stars) {
        star.phase += (star.rate * deltaMs) / 1000;
        // Never fully dark: a star that blinks out reads as a dead pixel.
        target.globalAlpha = 0.35 + 0.4 * (0.5 + 0.5 * Math.sin(star.phase));
        target.fillStyle = star.r > 1.2 ? colours.accent : colours.ink;
        target.beginPath();
        target.arc(star.x, star.y, star.r, 0, Math.PI * 2);
        target.fill();
      }
      target.globalAlpha = 1;
    },
  };
}

/* -------------------------------------------------------------- particles */

interface Particle {
  x: number;
  y: number;
  dx: number;
  dy: number;
}

/** Drifting points with a line drawn between any two that come close: the
 *  "neural network" look, which is only a distance test. */
function createParticles(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let points: Particle[] = [];
  const linkDistance = 130;

  return {
    get count() {
      return points.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      points = Array.from({ length: countFor(w, h, 40, 90) }, () => ({
        x: random() * w,
        y: random() * h,
        dx: (random() - 0.5) * 22,
        dy: (random() - 0.5) * 22,
      }));
    },
    frame(target, deltaMs) {
      target.fillStyle = colours.canvas;
      target.globalAlpha = 1;
      target.fillRect(0, 0, width, height);

      const seconds = deltaMs / 1000;
      for (const point of points) {
        point.x += point.dx * seconds;
        point.y += point.dy * seconds;

        // Bounce rather than wrap: a point crossing the edge and reappearing
        // opposite reads as a glitch at this speed.
        if (point.x < 0 || point.x > width) point.dx *= -1;
        if (point.y < 0 || point.y > height) point.dy *= -1;
        point.x = Math.max(0, Math.min(width, point.x));
        point.y = Math.max(0, Math.min(height, point.y));
      }

      target.strokeStyle = colours.accent;
      target.lineWidth = 1;
      for (let i = 0; i < points.length; i += 1) {
        const from = points[i];
        if (!from) continue;

        for (let j = i + 1; j < points.length; j += 1) {
          const to = points[j];
          if (!to) continue;

          const distance = Math.hypot(from.x - to.x, from.y - to.y);
          if (distance > linkDistance) continue;

          target.globalAlpha = 0.18 * (1 - distance / linkDistance);
          target.beginPath();
          target.moveTo(from.x, from.y);
          target.lineTo(to.x, to.y);
          target.stroke();
        }
      }

      target.globalAlpha = 0.5;
      target.fillStyle = colours.accent;
      for (const point of points) {
        target.beginPath();
        target.arc(point.x, point.y, 1.6, 0, Math.PI * 2);
        target.fill();
      }
      target.globalAlpha = 1;
    },
  };
}

/* ------------------------------------------------------------------ blobs */

interface Blob {
  x: number;
  y: number;
  dx: number;
  dy: number;
  r: number;
}

function createBlobs(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let blobs: Blob[] = [];

  return {
    get count() {
      return blobs.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      const size = Math.min(w, h);
      blobs = Array.from({ length: 5 }, () => ({
        x: random() * w,
        y: random() * h,
        dx: (random() - 0.5) * 14,
        dy: (random() - 0.5) * 10,
        r: size * (0.18 + random() * 0.22),
      }));
    },
    frame(target, deltaMs) {
      target.fillStyle = colours.canvas;
      target.globalAlpha = 1;
      target.fillRect(0, 0, width, height);

      const seconds = deltaMs / 1000;
      target.fillStyle = colours.accent;
      for (const blob of blobs) {
        blob.x += blob.dx * seconds;
        blob.y += blob.dy * seconds;
        if (blob.x < -blob.r || blob.x > width + blob.r) blob.dx *= -1;
        if (blob.y < -blob.r || blob.y > height + blob.r) blob.dy *= -1;

        target.globalAlpha = 0.14;
        target.beginPath();
        target.arc(blob.x, blob.y, blob.r, 0, Math.PI * 2);
        target.fill();
      }
      target.globalAlpha = 1;
    },
  };
}

/* ---------------------------------------------------------------- drizzle */

interface Drop {
  x: number;
  y: number;
  length: number;
  speed: number;
}

function createDrizzle(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let drops: Drop[] = [];
  const slant = 0.18;

  return {
    get count() {
      return drops.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      drops = Array.from({ length: countFor(w, h, 180, 320) }, () => ({
        x: random() * w,
        y: random() * h,
        length: 8 + random() * 16,
        speed: 320 + random() * 380,
      }));
    },
    frame(target, deltaMs) {
      target.fillStyle = colours.canvas;
      target.globalAlpha = 1;
      target.fillRect(0, 0, width, height);

      target.strokeStyle = colours.accent;
      target.lineWidth = 1;
      target.globalAlpha = 0.3;

      const seconds = deltaMs / 1000;
      for (const drop of drops) {
        drop.y += drop.speed * seconds;
        drop.x += drop.speed * slant * seconds;
        if (drop.y > height) {
          drop.y = -drop.length;
          drop.x = random() * width;
        }
        if (drop.x > width) drop.x = 0;

        target.beginPath();
        target.moveTo(drop.x, drop.y);
        target.lineTo(drop.x - drop.length * slant, drop.y - drop.length);
        target.stroke();
      }
      target.globalAlpha = 1;
    },
  };
}

/* ----------------------------------------------------------------- embers */

interface Ember {
  x: number;
  y: number;
  drift: number;
  speed: number;
  r: number;
  life: number;
}

function createEmbers(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let embers: Ember[] = [];

  const spawn = (ember: Ember, w: number, h: number) => {
    ember.x = random() * w;
    ember.y = h + random() * 40;
    ember.drift = (random() - 0.5) * 18;
    ember.speed = 18 + random() * 42;
    ember.r = 0.6 + random() * 1.8;
    ember.life = 0;
  };

  return {
    get count() {
      return embers.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      embers = Array.from({ length: countFor(w, h, 90, 180) }, () => {
        const ember: Ember = { x: 0, y: 0, drift: 0, speed: 0, r: 1, life: 0 };
        spawn(ember, w, h);
        // Staggered, or every ember rises in one wave from the bottom edge.
        ember.y = random() * h;
        ember.life = random();
        return ember;
      });
    },
    frame(target, deltaMs) {
      fade(target, colours.canvas, 0.18, width, height);

      const seconds = deltaMs / 1000;
      for (const ember of embers) {
        ember.y -= ember.speed * seconds;
        ember.x += ember.drift * seconds;
        ember.life += seconds * 0.12;
        if (ember.y < -10 || ember.life > 1) spawn(ember, width, height);

        target.globalAlpha = Math.max(0, 0.75 * (1 - ember.life));
        target.fillStyle = colours.accent;
        target.beginPath();
        target.arc(ember.x, ember.y, ember.r, 0, Math.PI * 2);
        target.fill();
      }
      target.globalAlpha = 1;
    },
  };
}

/* ----------------------------------------------------------------- petals */

interface Petal {
  x: number;
  y: number;
  r: number;
  speed: number;
  sway: number;
  phase: number;
}

function createPetals(colours: EffectColours, random: Random): EffectPainter {
  let width = 0;
  let height = 0;
  let petals: Petal[] = [];

  return {
    get count() {
      return petals.length;
    },
    resize(w, h) {
      width = w;
      height = h;
      petals = Array.from({ length: countFor(w, h, 55, 110) }, () => ({
        x: random() * w,
        y: random() * h,
        r: 2.5 + random() * 3.5,
        speed: 20 + random() * 34,
        sway: 12 + random() * 22,
        phase: random() * Math.PI * 2,
      }));
    },
    frame(target, deltaMs) {
      target.fillStyle = colours.canvas;
      target.globalAlpha = 1;
      target.fillRect(0, 0, width, height);

      const seconds = deltaMs / 1000;
      target.fillStyle = colours.accent;
      for (const petal of petals) {
        petal.phase += seconds;
        petal.y += petal.speed * seconds;
        petal.x += Math.sin(petal.phase) * petal.sway * seconds;
        if (petal.y > height + petal.r) {
          petal.y = -petal.r;
          petal.x = random() * width;
        }

        target.globalAlpha = 0.5;
        target.beginPath();
        if (target.ellipse) {
          target.ellipse(petal.x, petal.y, petal.r, petal.r * 0.55, petal.phase, 0, Math.PI * 2);
        } else {
          target.arc(petal.x, petal.y, petal.r, 0, Math.PI * 2);
        }
        target.fill();
      }
      target.globalAlpha = 1;
    },
  };
}

/* ---------------------------------------------------------------- factory */

const PAINTERS: Record<CanvasEffectId, (colours: EffectColours, random: Random) => EffectPainter> =
  {
    rain: createRain,
    stars: createStars,
    particles: createParticles,
    blobs: createBlobs,
    drizzle: createDrizzle,
    embers: createEmbers,
    petals: createPetals,
  };

export function createPainter(
  effect: CanvasEffectId,
  colours: EffectColours,
  random: Random = Math.random,
): EffectPainter {
  return PAINTERS[effect](colours, random);
}
