/**
 * Things that float above the page: modals, confirmations, and the little menu
 * an overflow button opens.
 *
 * All three share one rule — they close on Escape, they close on a click
 * outside, and they are measured so they cannot render past the edge of the
 * window. Getting that wrong is how a dialog ends up with its buttons off
 * screen on a small display.
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

import Icon, { type IconName } from './Icon';
import { Button } from './primitives';

/** Escape closes the topmost overlay. */
function useEscape(onClose: () => void) {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);
}

/**
 * A modal.
 *
 * `size` exists because the wizard needs room and a confirmation does not; a
 * confirmation in a 900px box reads as more serious than it is.
 */
export function Modal({
  title,
  description,
  size = 'md',
  onClose,
  children,
  footer,
}: {
  title: string;
  description?: string;
  size?: 'sm' | 'md' | 'lg';
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  useEscape(onClose);

  const widths = { sm: 'max-w-[440px]', md: 'max-w-[560px]', lg: 'max-w-[880px]' };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-start justify-center overflow-y-auto bg-black/60 p-4 sm:p-8"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        className={`my-auto flex max-h-[calc(100vh-64px)] w-full ${widths[size]} flex-col overflow-hidden rounded-[14px] border border-edge bg-surface shadow-[0_24px_64px_rgba(0,0,0,0.6)]`}
      >
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-edge px-5 py-4">
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold">{title}</h2>
            {description && <p className="mt-0.5 text-[13px] text-muted">{description}</p>}
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="shrink-0 rounded-[8px] p-1 text-muted hover:bg-raised hover:text-ink"
          >
            <Icon name="close" size={16} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>

        {footer && (
          <div className="flex shrink-0 items-center justify-end gap-2 border-t border-edge px-5 py-3">
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * "Are you sure?" — asked only for things that cannot be undone.
 *
 * Asking about a reversible action trains people to click through the dialog
 * that matters.
 */
export function ConfirmDialog({
  title,
  description,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: {
  title: string;
  description?: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      title={title}
      description={description}
      size="sm"
      onClose={onCancel}
      footer={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p className="text-[13px] text-muted">
        {danger ? 'This cannot be undone.' : 'Confirm to continue.'}
      </p>
    </Modal>
  );
}

export interface MenuItem {
  id: string;
  label: string;
  icon?: IconName;
  disabled?: boolean;
  reason?: string;
  danger?: boolean;
  run: () => void;
}

/**
 * The overflow menu.
 *
 * Anchored to the button that opened it and flipped when it would run off the
 * bottom or the right of the window.
 */
export function Menu({
  items,
  anchor,
  onClose,
}: {
  items: MenuItem[];
  anchor: DOMRect;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement | null>(null);
  const [placement, setPlacement] = useState<{ left: number; top: number } | null>(null);
  useEscape(onClose);

  useLayoutEffect(() => {
    const element = menu.current;
    if (!element) return;
    const { width, height } = element.getBoundingClientRect();
    const margin = 8;
    // Right-aligned to the anchor, because an overflow button sits at the end
    // of a row and a menu growing rightwards from it leaves the window.
    const left = Math.max(
      margin,
      Math.min(anchor.right - width, window.innerWidth - width - margin),
    );
    const below = anchor.bottom + 4;
    const preferred =
      below + height + margin > window.innerHeight ? anchor.top - height - 4 : below;

    // Clamped at both ends, not only the top. Flipping is enough while the
    // anchor is on screen; it is not once the anchor has been scrolled past,
    // and a menu hanging below the window cannot be reached at all.
    const lowest = Math.max(margin, window.innerHeight - height - margin);
    setPlacement({ left, top: Math.min(Math.max(margin, preferred), lowest) });
  }, [anchor, items.length]);

  useEffect(() => {
    function onPointerDown(event: MouseEvent) {
      if (!menu.current?.contains(event.target as Node)) onClose();
    }
    window.addEventListener('mousedown', onPointerDown, true);
    window.addEventListener('resize', onClose);
    window.addEventListener('scroll', onClose, true);
    return () => {
      window.removeEventListener('mousedown', onPointerDown, true);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('scroll', onClose, true);
    };
  }, [onClose]);

  return (
    <div
      ref={menu}
      role="menu"
      style={{
        left: placement?.left,
        top: placement?.top,
        visibility: placement ? 'visible' : 'hidden',
      }}
      className="fixed z-[70] min-w-[200px] rounded-[10px] border border-edge bg-overlay p-1 shadow-[0_12px_36px_rgba(0,0,0,0.5)]"
    >
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          title={item.reason}
          onClick={() => {
            onClose();
            item.run();
          }}
          className={`flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[13px] disabled:pointer-events-none disabled:opacity-40 ${
            item.danger ? 'text-danger hover:bg-danger/10' : 'text-ink hover:bg-raised'
          }`}
        >
          {item.icon && <Icon name={item.icon} size={14} />}
          {item.label}
        </button>
      ))}
    </div>
  );
}

/**
 * The state and the anchor an overflow menu needs, so a caller writes
 * `const menu = useMenu()` instead of three `useState`s.
 */
export function useMenu() {
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  return {
    anchor,
    open: (event: React.MouseEvent) => setAnchor(event.currentTarget.getBoundingClientRect()),
    openAt: (rect: DOMRect) => setAnchor(rect),
    close: () => setAnchor(null),
  };
}
