/**
 * The 4px strip between two panes.
 *
 * Pointer capture rather than window listeners: the drag keeps working when the
 * pointer crosses the editor, the webview, or leaves the window entirely, and
 * it ends even if the button is released somewhere that never fires a mouseup
 * back to us.
 *
 * It is also a real separator for the keyboard — arrow keys resize, Home and
 * End jump to the extremes — because a pane that can only be sized by dragging
 * cannot be sized without a mouse.
 */
export default function ResizeHandle({
  orientation,
  label,
  value,
  onResize,
  onDoubleClick,
}: {
  /** `vertical` is a vertical bar that resizes horizontally. */
  orientation: 'vertical' | 'horizontal';
  label: string;
  /** The current size, so keyboard steps have something to move. */
  value: number;
  /** Called with the pointer's position along the axis, or a stepped size. */
  onResize: (next: number, source: 'pointer' | 'keyboard') => void;
  onDoubleClick?: () => void;
}) {
  const vertical = orientation === 'vertical';

  function onPointerDown(event: React.PointerEvent<HTMLDivElement>) {
    // Only the primary button. A right-click here would otherwise start a drag
    // that no button release ends.
    if (event.button !== 0) return;
    event.preventDefault();
    const element = event.currentTarget;
    element.setPointerCapture(event.pointerId);
    element.dataset.dragging = 'true';
  }

  function onPointerMove(event: React.PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.dataset.dragging !== 'true') return;
    onResize(vertical ? event.clientX : event.clientY, 'pointer');
  }

  function onPointerUp(event: React.PointerEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    delete element.dataset.dragging;
    if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
  }

  function onKeyDown(event: React.KeyboardEvent) {
    const step = event.shiftKey ? 40 : 8;
    const decrease = vertical ? 'ArrowLeft' : 'ArrowUp';
    const increase = vertical ? 'ArrowRight' : 'ArrowDown';

    if (event.key === decrease) {
      event.preventDefault();
      onResize(value - step, 'keyboard');
    } else if (event.key === increase) {
      event.preventDefault();
      onResize(value + step, 'keyboard');
    } else if (event.key === 'Home') {
      event.preventDefault();
      onResize(0, 'keyboard');
    } else if (event.key === 'End') {
      event.preventDefault();
      onResize(Number.MAX_SAFE_INTEGER, 'keyboard');
    }
  }

  return (
    <div
      role="separator"
      aria-label={label}
      aria-orientation={vertical ? 'vertical' : 'horizontal'}
      aria-valuenow={Math.round(value)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      className={`vs-handle z-10 shrink-0 ${
        vertical ? 'w-1 cursor-col-resize' : 'h-1 cursor-row-resize'
      }`}
    />
  );
}
