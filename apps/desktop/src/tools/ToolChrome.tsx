/**
 * The furniture every sidebar tool is built from.
 *
 * One implementation of the header, the section label, the row and the empty
 * state, so that eight panels cannot drift into eight slightly different
 * paddings. Nothing here is a card: a tool fills its column and is separated
 * from its neighbours by a 1px rule, because a rounded box inside a rounded
 * panel inside a rounded window is the look this interface is not.
 */
import type { ReactNode } from 'react';

import Icon, { type IconName } from '../ui/Icon';

/** The 36px bar at the top of a tool. */
export function ToolHeader({ title, actions }: { title: string; actions?: ReactNode }) {
  return (
    <div
      className="flex shrink-0 items-center justify-between gap-2 border-b border-edge px-2.5"
      style={{ height: 'var(--h-panel-header)' }}
    >
      <h2 className="truncate text-[11px] font-semibold tracking-wide text-muted uppercase">
        {title}
      </h2>
      {actions !== undefined && <div className="flex shrink-0 items-center gap-0.5">{actions}</div>}
    </div>
  );
}

/** A 24px icon button for a tool header. */
export function ToolAction({
  icon,
  label,
  onClick,
  disabled = false,
}: {
  icon: IconName;
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="grid h-6 w-6 place-items-center rounded-[3px] text-muted hover:bg-raised hover:text-ink disabled:cursor-not-allowed disabled:text-faint disabled:hover:bg-transparent"
    >
      <Icon name={icon} size={14} />
    </button>
  );
}

/** A group label inside a tool. */
export function ToolSection({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="border-b border-edge/60 last:border-b-0">
      <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-semibold tracking-wide text-faint uppercase">
        {label}
      </div>
      {children}
    </div>
  );
}

/**
 * One line in a tool.
 *
 * 27px, which is the height at which a list of twenty is readable without
 * scrolling and a single row still has room for a 15px icon.
 */
export function ToolRow({
  active = false,
  onClick,
  title,
  children,
}: {
  active?: boolean;
  onClick?: () => void;
  title?: string;
  children: ReactNode;
}) {
  const shared =
    'flex w-full items-center gap-2 px-2.5 text-left text-[12.5px] leading-none transition-colors duration-100';
  const look = active ? 'bg-raised text-ink' : 'text-muted hover:bg-raised/60 hover:text-ink';

  if (onClick === undefined) {
    return (
      <div className={`${shared} ${look}`} style={{ height: 'var(--h-row)' }} title={title}>
        {children}
      </div>
    );
  }

  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={`${shared} ${look}`}
      style={{ height: 'var(--h-row)' }}
    >
      {children}
    </button>
  );
}

/**
 * A key and its value on one line.
 *
 * The value is allowed to shrink and truncate; the label is not. A row that
 * truncates its label to fit a long value has thrown away the half that says
 * what it is.
 */
export function ToolFact({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div
      className="flex items-center gap-2 px-2.5 text-[12.5px]"
      style={{ height: 'var(--h-row)' }}
    >
      <span className="shrink-0 text-muted">{label}</span>
      <span className="min-w-0 flex-1 truncate text-right text-ink tabular">{value}</span>
    </div>
  );
}

/**
 * What a tool shows when there is nothing to show.
 *
 * A sentence and, where there is one, the action that would fix it. No
 * illustration and no exclamation mark: an empty list is a normal state, not an
 * error, and dressing it up wastes the space the content will need.
 */
export function ToolEmpty({
  message,
  action,
}: {
  message: string;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className="px-3 py-6 text-center">
      <p className="text-[12px] text-muted">{message}</p>
      {action !== undefined && (
        <button
          type="button"
          onClick={action.onClick}
          className="mt-2 h-[26px] rounded-[5px] border border-edge bg-raised px-2.5 text-[12px] text-ink hover:bg-overlay"
        >
          {action.label}
        </button>
      )}
    </div>
  );
}

/** The body of a tool: scrolls on its own, never the page. */
export function ToolBody({ children }: { children: ReactNode }) {
  return <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>;
}
