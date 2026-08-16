/**
 * The 36px tab strip above the workspace.
 *
 * Tabs sit flush against the surface below them, sharing its background when
 * active, so the active tab reads as the front of the pane rather than as a
 * button floating above it. That is the whole difference between a tab strip
 * and a row of pills.
 */
import Icon, { type IconName } from '../ui/Icon';

export interface WorkspaceTab {
  id: string;
  label: string;
  icon: IconName;
  /** A project mid-transition, or a file with unsaved changes. */
  dot?: boolean;
  /** Omitted for tabs that are part of the workspace rather than opened into it. */
  onClose?: () => void;
}

export default function WorkspaceTabs({
  tabs,
  active,
  onSelect,
}: {
  tabs: WorkspaceTab[];
  active: string;
  onSelect: (id: string) => void;
}) {
  if (tabs.length === 0) return null;

  return (
    <div
      role="tablist"
      className="flex shrink-0 items-stretch overflow-x-auto border-b border-edge bg-surface"
      style={{ height: 'var(--h-tab)' }}
    >
      {tabs.map((tab) => {
        const selected = tab.id === active;
        return (
          <div
            key={tab.id}
            className={`group relative flex min-w-[90px] max-w-[200px] shrink-0 items-center border-r border-edge ${
              selected ? 'bg-canvas text-ink' : 'text-muted hover:bg-raised/50'
            }`}
          >
            {/* A 1px line along the top of the active tab rather than a filled
                highlight: it marks the tab without repainting it. */}
            {selected && (
              <span aria-hidden className="absolute inset-x-0 top-0 h-[1.5px] bg-accent" />
            )}

            <button
              type="button"
              role="tab"
              aria-selected={selected}
              onClick={() => onSelect(tab.id)}
              title={tab.label}
              className="flex min-w-0 flex-1 items-center gap-1.5 px-2.5 text-[12.5px]"
            >
              <Icon name={tab.icon} size={13} />
              <span className="min-w-0 flex-1 truncate text-left">{tab.label}</span>
              {tab.dot === true && (
                <span aria-hidden className="h-[6px] w-[6px] shrink-0 rounded-full bg-warn" />
              )}
            </button>

            {tab.onClose !== undefined && (
              <button
                type="button"
                aria-label={`Close ${tab.label}`}
                title={`Close ${tab.label}`}
                onClick={tab.onClose}
                // Visible on the active tab and on hover, so a strip of eight
                // tabs is not eight close buttons competing with the labels.
                className={`mr-1 grid h-5 w-5 shrink-0 place-items-center rounded-[3px] text-faint hover:bg-overlay hover:text-ink ${
                  selected ? '' : 'opacity-0 group-hover:opacity-100'
                }`}
              >
                <Icon name="close" size={11} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
