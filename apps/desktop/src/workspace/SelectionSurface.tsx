/**
 * The scroll area that a selection rectangle can be dragged across.
 *
 * Wraps whatever rows it is given and takes care of the parts that are only
 * correct at runtime: measuring rows against the *content* box rather than the
 * viewport, scrolling itself when the pointer nears an edge, and taking every
 * listener back down when the drag ends — including when it ends because the
 * window lost focus or the component went away mid-drag.
 *
 * The window listeners are registered exactly once, and every callback they
 * need is read from a ref. An earlier version listed those callbacks as effect
 * dependencies, which meant a caller passing an inline arrow — as both callers
 * do — re-ran the effect on the first selection change, and its cleanup tore
 * down the drag that was still happening. The rectangle vanished after one
 * frame. Listeners that outlive a render must not depend on one.
 *
 * The geometry lives in `rubberBand.ts` and is tested there.
 */
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';

import {
  autoScrollAmount,
  combineRubberBand,
  normaliseRect,
  passedThreshold,
  rowsInRect,
  startsRubberBand,
  type Rect,
  type RowBox,
} from './rubberBand';

export default function SelectionSurface({
  children,
  className = '',
  onSelectPaths,
  onClearSelection,
  currentPaths,
  onContextMenu,
}: {
  children: ReactNode;
  className?: string;
  /** The rows the rectangle currently covers, combined with what came before. */
  onSelectPaths: (paths: string[]) => void;
  /** A plain click on empty space. */
  onClearSelection: () => void;
  /** What is selected right now, snapshotted when a ctrl-drag begins. */
  currentPaths: string[];
  onContextMenu?: (event: React.MouseEvent) => void;
}) {
  const surface = useRef<HTMLDivElement | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);

  // Read by handlers that are registered once and live for the life of the
  // component. Assigning during render keeps them current without making the
  // listeners depend on a render.
  const selectRef = useRef(onSelectPaths);
  selectRef.current = onSelectPaths;
  const clearRef = useRef(onClearSelection);
  clearRef.current = onClearSelection;
  const currentRef = useRef(currentPaths);
  currentRef.current = currentPaths;

  /**
   * Everything the in-flight drag needs.
   *
   * A ref rather than state: the pointer handlers run many times a second and
   * must not each schedule a render, and the auto-scroll loop reads the latest
   * pointer position from here rather than from a stale closure.
   */
  const drag = useRef<{
    anchorX: number;
    anchorY: number;
    pointerX: number;
    pointerY: number;
    /** Viewport coordinate, for the auto-scroll edge test. */
    clientY: number;
    additive: boolean;
    before: string[];
    active: boolean;
    frame: number | null;
  } | null>(null);

  /** Every row's box, in content coordinates. Measured when the drag starts. */
  const rows = useRef<RowBox[]>([]);

  const measure = useCallback((): RowBox[] => {
    const element = surface.current;
    if (!element) return [];
    const bounds = element.getBoundingClientRect();

    return [...element.querySelectorAll<HTMLElement>('[data-row][data-path]')].map((row) => {
      const box = row.getBoundingClientRect();
      return {
        path: row.dataset.path ?? '',
        // Viewport position, minus where the container starts, plus how far it
        // is scrolled: the row's position in the content, which does not move
        // when the list scrolls under the rectangle.
        top: box.top - bounds.top + element.scrollTop,
        bottom: box.bottom - bounds.top + element.scrollTop,
        left: box.left - bounds.left + element.scrollLeft,
        right: box.right - bounds.left + element.scrollLeft,
      };
    });
  }, []);

  useEffect(() => {
    function update() {
      const state = drag.current;
      if (!state) return;
      const box = normaliseRect(state.anchorX, state.anchorY, state.pointerX, state.pointerY);
      setRect(box);
      selectRef.current(
        combineRubberBand(state.before, rowsInRect(rows.current, box), state.additive),
      );
    }

    function stop() {
      const state = drag.current;
      if (state?.frame !== null && state?.frame !== undefined) {
        cancelAnimationFrame(state.frame);
      }
      drag.current = null;
      rows.current = [];
      setRect(null);
    }

    /** Scroll while the pointer sits near an edge, and keep the box growing. */
    function step() {
      const element = surface.current;
      const state = drag.current;
      if (!element || !state || !state.active) return;

      const bounds = element.getBoundingClientRect();
      const amount = autoScrollAmount(state.clientY, bounds.top, bounds.bottom);
      if (amount !== 0) {
        const before = element.scrollTop;
        element.scrollTop += amount;
        // The pointer has not moved, but the content under it has, so the far
        // corner of the rectangle follows the content.
        state.pointerY += element.scrollTop - before;
        update();
      }
      state.frame = requestAnimationFrame(step);
    }

    function onMove(event: MouseEvent) {
      const element = surface.current;
      const state = drag.current;
      if (!element || !state) return;

      const bounds = element.getBoundingClientRect();
      state.pointerX = event.clientX - bounds.left + element.scrollLeft;
      state.pointerY = event.clientY - bounds.top + element.scrollTop;
      state.clientY = event.clientY;

      if (!state.active) {
        // Below the threshold this is still a click, not a rectangle.
        if (!passedThreshold(state.anchorX, state.anchorY, state.pointerX, state.pointerY)) return;
        state.active = true;
        rows.current = measure();
        state.frame = requestAnimationFrame(step);
      }

      // Without this the browser selects the labels the box passes over.
      event.preventDefault();
      update();
    }

    function onUp() {
      const state = drag.current;
      // A press that never became a drag is a click on empty space.
      if (state && !state.active && !state.additive) clearRef.current();
      stop();
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    // A drag interrupted by Alt-Tab, a dialog, or an HTML5 drag must not leave
    // a rectangle painted over the tree with no way to clear it.
    window.addEventListener('blur', stop);
    window.addEventListener('dragstart', stop);

    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      window.removeEventListener('blur', stop);
      window.removeEventListener('dragstart', stop);
      // Unmounting mid-drag: cancel the frame loop rather than leaving it
      // calling into a component that is gone.
      stop();
    };
  }, [measure]);

  function onMouseDown(event: React.MouseEvent) {
    // Only the primary button, and only on empty space: a press on a row is
    // the start of a click or a drag of that row.
    if (event.button !== 0 || !startsRubberBand(event.target)) return;

    const element = surface.current;
    if (!element) return;
    const bounds = element.getBoundingClientRect();
    const x = event.clientX - bounds.left + element.scrollLeft;
    const y = event.clientY - bounds.top + element.scrollTop;
    const additive = event.ctrlKey || event.metaKey;

    drag.current = {
      anchorX: x,
      anchorY: y,
      pointerX: x,
      pointerY: y,
      clientY: event.clientY,
      additive,
      // Snapshotted, so sweeping the box back and forth does not toggle rows.
      before: additive ? [...currentRef.current] : [],
      active: false,
      frame: null,
    };
  }

  return (
    <div
      ref={surface}
      onMouseDown={onMouseDown}
      onContextMenu={onContextMenu}
      className={`relative ${className}`}
    >
      {children}

      {rect && (
        <div
          aria-hidden
          data-rubber-band
          className="pointer-events-none absolute border border-accent bg-accent/20"
          style={{
            left: rect.left,
            top: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
          }}
        />
      )}
    </div>
  );
}
