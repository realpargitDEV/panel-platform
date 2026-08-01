/**
 * The panel along the bottom: Problems, Output, Terminal, Logs.
 *
 * Which tab is showing is remembered across sessions, and the whole panel
 * mounts once — switching tabs hides one child and shows another rather than
 * unmounting it, so a scrolled-through transcript is still scrolled when you
 * come back to it.
 */
import type { ReactNode } from 'react';

import Icon, { type IconName } from './Icon';
import type { PanelTab } from './layout';

const TABS: { id: PanelTab; label: string; icon: IconName }[] = [
  { id: 'problems', label: 'Problems', icon: 'warning' },
  { id: 'output', label: 'Output', icon: 'output' },
  { id: 'terminal', label: 'Terminal', icon: 'terminal' },
  { id: 'logs', label: 'Logs', icon: 'file' },
];

export default function BottomPanel({
  tab,
  onSelect,
  onClose,
  problemCount,
  children,
}: {
  tab: PanelTab;
  onSelect: (tab: PanelTab) => void;
  onClose: () => void;
  problemCount: number;
  /** One child per tab, keyed by tab id. */
  children: Record<PanelTab, ReactNode>;
}) {
  return (
    <section
      aria-label="Panel"
      className="flex min-h-0 flex-1 flex-col border-t border-vs-border bg-vs-panel"
    >
      <div className="flex h-[35px] shrink-0 items-center gap-3 border-b border-vs-border px-3">
        <div role="tablist" aria-label="Panel sections" className="flex flex-1 items-center gap-3">
          {TABS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              role="tab"
              aria-selected={tab === entry.id}
              onClick={() => onSelect(entry.id)}
              className={`flex h-[35px] items-center gap-1.5 border-b-2 text-[11px] tracking-wide uppercase ${
                tab === entry.id
                  ? 'border-vs-text text-white'
                  : 'border-transparent text-vs-dim hover:text-vs-text'
              }`}
            >
              <Icon name={entry.icon} size={14} />
              {entry.label}
              {entry.id === 'problems' && problemCount > 0 && (
                <span className="grid h-4 min-w-4 place-items-center rounded-full bg-vs-badge px-1 text-[10px] font-semibold text-white">
                  {problemCount > 99 ? '99+' : problemCount}
                </span>
              )}
            </button>
          ))}
        </div>

        <button
          type="button"
          onClick={onClose}
          title="Close Panel (Ctrl+J)"
          aria-label="Close Panel"
          className="grid h-5 w-5 place-items-center rounded-[3px] text-vs-text hover:bg-white/10"
        >
          <Icon name="close" size={14} />
        </button>
      </div>

      {TABS.map((entry) => (
        <div
          key={entry.id}
          role="tabpanel"
          hidden={tab !== entry.id}
          className={tab === entry.id ? 'min-h-0 flex-1 overflow-auto' : 'hidden'}
        >
          {children[entry.id]}
        </div>
      ))}
    </section>
  );
}
