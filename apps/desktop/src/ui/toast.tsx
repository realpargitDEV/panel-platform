/**
 * Toasts.
 *
 * A module-level store rather than a context provider: there is exactly one
 * stack of notifications for the life of the window, every screen raises them,
 * and a provider would only add a place to forget to mount. The same shape the
 * update store already uses.
 *
 * Success toasts dismiss themselves; errors do not. An error nobody read is an
 * error that will be reported as "it just didn't work".
 */
import { useSyncExternalStore } from 'react';

import Icon, { type IconName } from './Icon';

export type ToastTone = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  description?: string;
  /** An optional single action, e.g. "Retry". Dismisses when pressed. */
  action?: { label: string; run: () => void };
}

const DISMISS_AFTER_MS = 4000;

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

export function dismissToast(id: number): void {
  toasts = toasts.filter((toast) => toast.id !== id);
  emit();
}

function push(
  tone: ToastTone,
  title: string,
  description?: string,
  action?: Toast['action'],
): number {
  const id = nextId++;
  toasts = [...toasts, { id, tone, title, description, action }];
  emit();
  // Errors stay until dismissed. Everything else is a confirmation the user
  // does not need to act on.
  if (tone !== 'error') {
    setTimeout(() => dismissToast(id), DISMISS_AFTER_MS);
  }
  return id;
}

export const toast = {
  success: (title: string, description?: string) => push('success', title, description),
  error: (title: string, description?: string, action?: Toast['action']) =>
    push('error', title, description, action),
  info: (title: string, description?: string) => push('info', title, description),
};

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function snapshot(): Toast[] {
  return toasts;
}

/** Mounted once, by the shell. */
export function ToastHost() {
  const items = useSyncExternalStore(subscribe, snapshot, snapshot);
  if (items.length === 0) return null;

  const icons: Record<ToastTone, IconName> = {
    success: 'check-circle',
    error: 'alert',
    info: 'info',
  };
  const colours: Record<ToastTone, string> = {
    success: 'text-ok',
    error: 'text-danger',
    info: 'text-accent',
  };

  return (
    <div
      role="region"
      aria-label="Notifications"
      aria-live="polite"
      className="pointer-events-none fixed right-4 bottom-4 z-[80] flex w-[min(380px,calc(100vw-32px))] flex-col gap-2"
    >
      {items.map((item) => (
        <div
          key={item.id}
          className="toast-enter pointer-events-auto flex items-start gap-3 rounded-[12px] border border-edge bg-overlay px-3.5 py-3 shadow-[0_10px_30px_rgba(0,0,0,0.45)]"
        >
          <span className={`mt-0.5 ${colours[item.tone]}`}>
            <Icon name={icons[item.tone]} size={16} />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-[13px] font-medium text-ink">{item.title}</p>
            {item.description && (
              <p className="mt-0.5 text-[12px] break-words text-muted">{item.description}</p>
            )}
            {item.action && (
              <button
                type="button"
                onClick={() => {
                  dismissToast(item.id);
                  item.action?.run();
                }}
                className="mt-1.5 text-[12px] font-medium text-accent hover:underline"
              >
                {item.action.label}
              </button>
            )}
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => dismissToast(item.id)}
            className="mt-0.5 shrink-0 text-faint hover:text-ink"
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
