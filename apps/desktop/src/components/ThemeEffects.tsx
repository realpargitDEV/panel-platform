import { useEffect, useRef } from 'react';

import type { MotionLevel } from '../lib/appearance';
import { createPainter, type EffectColours } from '../lib/themes/effects';
import { isCanvasEffect, type EffectId } from '../lib/themes/types';

/**
 * The background a theme asks for, rendered once, behind everything.
 *
 * Two kinds of effect arrive here and only one of them costs anything. The CSS
 * effects — scan lines, grids, aurora, grain — are a `data-effect` attribute
 * and a stylesheet rule: they paint and then stop, so they need no loop and no
 * supervision. The canvas effects animate, and everything below exists to make
 * sure they only animate while somebody is actually looking at them.
 *
 * The frame loop stops when the window is hidden, when it loses focus, and
 * whenever motion is anything but `full`. An application that manages
 * containers is left open all day, often minimised; a code-rain canvas burning
 * a core behind a minimised window is how a theme gets a laptop's fan blamed on
 * the app.
 */
export default function ThemeEffects({
  effect,
  motion,
}: {
  effect?: EffectId;
  motion: MotionLevel;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // Canvas effects are animation, so `reduced` and `off` remove them entirely
  // rather than slowing them down. The CSS ones stay: a scan line is what the
  // theme looks like, not something it does.
  const drawn = effect !== undefined && isCanvasEffect(effect);
  const animated = drawn && motion === 'full';

  useEffect(() => {
    if (!animated || effect === undefined || !isCanvasEffect(effect)) return;

    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;

    // The theme's own colours, read from the same custom properties everything
    // else uses — so an effect can never disagree with the palette it sits
    // behind, including when an explicit accent overrides the theme's.
    const styles = getComputedStyle(document.documentElement);
    const read = (name: string, fallback: string) =>
      styles.getPropertyValue(name).trim() || fallback;

    const colours: EffectColours = {
      accent: read('--color-accent', '#3b82f6'),
      canvas: read('--color-canvas', '#0e0e10'),
      ink: read('--color-ink', '#e8e8ec'),
    };

    const painter = createPainter(effect, colours);

    // Capped at 2: beyond that the pixel count doubles again for a difference
    // nobody can see in a blurred background.
    const ratio = Math.min(2, window.devicePixelRatio || 1);

    const resize = () => {
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (width === 0 || height === 0) return;

      canvas.width = Math.floor(width * ratio);
      canvas.height = Math.floor(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      painter.resize(width, height);
    };

    resize();

    let frame = 0;
    let last = performance.now();
    let running = false;

    const tick = (now: number) => {
      // Clamped: a tab that was throttled hands back a delta of seconds, and an
      // unclamped step would teleport every particle across the screen at once.
      const delta = Math.min(50, now - last);
      last = now;
      painter.frame(context, delta);
      frame = requestAnimationFrame(tick);
    };

    const start = () => {
      if (running) return;
      running = true;
      last = performance.now();
      frame = requestAnimationFrame(tick);
    };

    const stop = () => {
      if (!running) return;
      running = false;
      cancelAnimationFrame(frame);
    };

    const onVisibility = () => {
      if (document.hidden) stop();
      else start();
    };

    if (!document.hidden) start();

    window.addEventListener('resize', resize);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('blur', stop);
    window.addEventListener('focus', start);

    return () => {
      stop();
      window.removeEventListener('resize', resize);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('blur', stop);
      window.removeEventListener('focus', start);
    };
  }, [animated, effect]);

  if (!effect) return null;
  // A canvas effect with the loop switched off is nothing at all — better an
  // absent layer than an empty one sitting over the canvas.
  if (drawn && !animated) return null;

  return (
    <div className="theme-effects" data-effect={drawn ? undefined : effect} aria-hidden="true">
      {animated && <canvas ref={canvasRef} />}
    </div>
  );
}
