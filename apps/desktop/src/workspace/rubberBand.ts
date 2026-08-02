/**
 * The selection rectangle.
 *
 * Geometry only, so the parts that are easy to get wrong — a drag that goes up
 * and to the left, a tree that has been scrolled, a row that is only half
 * inside the box — are testable without a browser.
 *
 * Everything here works in the scroll container's *content* coordinates: the
 * pointer position plus how far the container is scrolled. Storing viewport
 * coordinates instead makes the rectangle slide off the rows the moment the
 * list auto-scrolls, which is exactly when the user is watching it.
 */

export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** A row's box, in the same content coordinates as the rectangle. */
export interface RowBox {
  path: string;
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/**
 * The box between two corners, whichever way the drag went.
 *
 * A drag up and to the left produces a negative width if the points are used
 * as-is, and every intersection test then reports nothing selected.
 */
export function normaliseRect(
  anchorX: number,
  anchorY: number,
  pointerX: number,
  pointerY: number,
): Rect {
  return {
    left: Math.min(anchorX, pointerX),
    right: Math.max(anchorX, pointerX),
    top: Math.min(anchorY, pointerY),
    bottom: Math.max(anchorY, pointerY),
  };
}

/**
 * Do two boxes overlap at all?
 *
 * Touching edges do not count. Dragging a zero-height line along the boundary
 * between two rows should not select both.
 */
export function intersects(a: Rect, b: Rect): boolean {
  return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
}

/**
 * Which rows the rectangle touches.
 *
 * Rows are full-width in a tree, so the horizontal test rarely matters — but it
 * is applied anyway, because the preview reuses this for a grid where it does.
 */
export function rowsInRect(rows: RowBox[], rect: Rect): string[] {
  return rows.filter((row) => intersects(rect, row)).map((row) => row.path);
}

/** How far the box has to be dragged before it counts as one. */
export const DRAG_THRESHOLD = 4;

/**
 * Has the pointer moved far enough to mean a rectangle?
 *
 * Without this every click on empty space starts and ends a one-pixel drag,
 * which flickers a rectangle and clears the selection twice.
 */
export function passedThreshold(
  anchorX: number,
  anchorY: number,
  pointerX: number,
  pointerY: number,
): boolean {
  return (
    Math.abs(pointerX - anchorX) >= DRAG_THRESHOLD || Math.abs(pointerY - anchorY) >= DRAG_THRESHOLD
  );
}

/** The band within which the list starts scrolling itself. */
export const AUTO_SCROLL_EDGE = 28;
/** The fastest it scrolls, in pixels per frame. */
export const AUTO_SCROLL_MAX = 16;

/**
 * How much to scroll while the pointer sits near an edge.
 *
 * Proportional to how deep into the edge band the pointer is, so it creeps near
 * the boundary and moves quickly at the very edge. Zero in the middle, negative
 * upwards.
 */
export function autoScrollAmount(
  pointerY: number,
  viewportTop: number,
  viewportBottom: number,
): number {
  if (viewportBottom <= viewportTop) return 0;

  const fromTop = pointerY - viewportTop;
  if (fromTop < AUTO_SCROLL_EDGE) {
    const depth = Math.min(AUTO_SCROLL_EDGE, AUTO_SCROLL_EDGE - fromTop);
    return -Math.ceil((depth / AUTO_SCROLL_EDGE) * AUTO_SCROLL_MAX);
  }

  const fromBottom = viewportBottom - pointerY;
  if (fromBottom < AUTO_SCROLL_EDGE) {
    const depth = Math.min(AUTO_SCROLL_EDGE, AUTO_SCROLL_EDGE - fromBottom);
    return Math.ceil((depth / AUTO_SCROLL_EDGE) * AUTO_SCROLL_MAX);
  }

  return 0;
}

/**
 * Should a press here begin a rectangle?
 *
 * Only on empty space. A press that lands on a row is the start of a click or a
 * drag of that row, and turning it into a rectangle would make files
 * undraggable.
 */
export function startsRubberBand(target: unknown): boolean {
  if (target === null || typeof target !== 'object') return false;
  const candidate = target as { closest?: (selector: string) => unknown };
  if (typeof candidate.closest !== 'function') return false;
  return candidate.closest('[data-row], button, input, [role="treeitem"]') === null;
}

/**
 * The rows selected by a rectangle, combined with what was already selected.
 *
 * Plain: the rectangle is the selection. Ctrl: the rectangle is added to what
 * was there when the drag began — measured against that snapshot rather than
 * the running selection, so sweeping back and forth does not toggle rows on and
 * off as the box passes over them.
 */
export function combineRubberBand(before: string[], inRect: string[], additive: boolean): string[] {
  if (!additive) return [...inRect];
  return [...new Set([...before, ...inRect])];
}
