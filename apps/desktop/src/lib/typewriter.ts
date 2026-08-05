/**
 * Console output that types itself out.
 *
 * The timing is a pure function of elapsed time, not a counter advanced by an
 * interval. That matters for correctness rather than tidiness: a counter drifts
 * whenever the tab is throttled or a frame is late, and a long line would then
 * finish at a different moment depending on how busy the machine was. Asking
 * "how much should be visible by now" makes a dropped frame catch up instead.
 */

import { useEffect, useState } from 'react';

/** Fast enough to read as typing rather than as waiting. A log line is not
 *  drama: at 30 characters a second an 80-column line lands in under three
 *  seconds, and the reveal never becomes the reason someone is waiting. */
export const CHARACTERS_PER_SECOND = 90;

/** How many characters of `total` should be showing after `elapsed` ms. */
export function charactersVisible(
  elapsed: number,
  total: number,
  charactersPerSecond = CHARACTERS_PER_SECOND,
): number {
  if (!Number.isFinite(elapsed) || elapsed <= 0) return 0;
  if (charactersPerSecond <= 0) return total;
  return Math.min(total, Math.floor((elapsed / 1000) * charactersPerSecond));
}

/** Whether this document wants motion at all — the app's Motion setting, with
 *  the operating system's preference able to veto it but never to enable it. */
export function motionAllowed(): boolean {
  if (typeof window === 'undefined' || typeof document === 'undefined') return false;
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return false;
  return document.documentElement.dataset.motion === 'full';
}

/**
 * Reveal `text` progressively, once, from the moment it first appears.
 *
 * Returns the whole string immediately when motion is off, so nothing has to
 * branch at the call site — and, importantly, so text is never withheld from
 * someone who asked for no animation.
 */
export function useTypewriter(text: string, enabled = true): { shown: string; done: boolean } {
  const [shown, setShown] = useState(() => (enabled && motionAllowed() ? '' : text));

  useEffect(() => {
    if (!enabled || !motionAllowed()) {
      setShown(text);
      return;
    }

    const started = performance.now();
    let frame = 0;

    const tick = () => {
      const count = charactersVisible(performance.now() - started, text.length);
      setShown(text.slice(0, count));
      if (count < text.length) frame = requestAnimationFrame(tick);
    };

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [text, enabled]);

  return { shown, done: shown.length >= text.length };
}
