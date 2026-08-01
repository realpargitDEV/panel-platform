/**
 * The tab strip.
 *
 * A tab shows the file's icon, its name, and — when there are unsaved changes —
 * a dot where the close button goes, which becomes the close button on hover.
 * That is VS Code's arrangement and it is the reason a dirty tab cannot be
 * closed by muscle memory without seeing that it was dirty.
 *
 * The strip scrolls sideways rather than wrapping: two rows of tabs would move
 * the editor down and back as files are opened.
 */
import { useEffect, useRef } from 'react';

import Icon from './Icon';
import { fileIconColor } from './fileIcons';
import { isDirty, tabLabel, type Buffer } from './tabs';

export default function EditorTabs({
  buffers,
  active,
  onSelect,
  onClose,
  onContextMenu,
}: {
  buffers: Buffer[];
  active: string | null;
  onSelect: (path: string) => void;
  onClose: (path: string) => void;
  onContextMenu: (path: string, event: React.MouseEvent) => void;
}) {
  const strip = useRef<HTMLDivElement | null>(null);

  // A tab activated from the palette or by closing its neighbour can be off
  // the visible part of the strip.
  useEffect(() => {
    if (active === null) return;
    strip.current
      ?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(active)}"]`)
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [active]);

  return (
    <div
      ref={strip}
      role="tablist"
      aria-label="Open editors"
      onWheel={(event) => {
        // A vertical wheel over the strip scrolls it sideways: there is
        // nothing to scroll vertically, and the alternative is a dead gesture.
        if (
          event.deltaY !== 0 &&
          event.currentTarget.scrollWidth > event.currentTarget.clientWidth
        ) {
          event.currentTarget.scrollLeft += event.deltaY;
        }
      }}
      className="vs-tabstrip flex h-[35px] shrink-0 items-stretch overflow-x-auto border-b border-vs-border bg-vs-tabbar"
    >
      {buffers.map((buffer) => {
        const isActive = buffer.path === active;
        const dirty = isDirty(buffer);

        return (
          <div
            key={buffer.path}
            data-tab={buffer.path}
            role="tab"
            aria-selected={isActive}
            tabIndex={isActive ? 0 : -1}
            title={buffer.path}
            onClick={() => onSelect(buffer.path)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                onSelect(buffer.path);
              }
            }}
            onAuxClick={(event) => {
              // Middle click closes, as it does everywhere else with tabs.
              if (event.button === 1) {
                event.preventDefault();
                onClose(buffer.path);
              }
            }}
            onContextMenu={(event) => onContextMenu(buffer.path, event)}
            className={`group/tab relative flex max-w-[220px] min-w-0 shrink-0 cursor-pointer items-center gap-1.5 border-r border-vs-border px-2.5 select-none ${
              isActive
                ? 'bg-vs-editor text-white'
                : 'bg-vs-tabbar text-vs-dim hover:bg-white/[0.04]'
            }`}
          >
            {/* VS Code marks the active tab with a line along its top edge. */}
            {isActive && <span className="absolute inset-x-0 top-0 h-px bg-accent" />}

            <span className="shrink-0" style={{ color: fileIconColor(tabLabel(buffer.path)) }}>
              <Icon name="file" size={15} />
            </span>
            <span className={`truncate text-[13px] ${buffer.readOnly ? 'italic' : ''}`}>
              {tabLabel(buffer.path)}
            </span>

            <button
              type="button"
              title={dirty ? 'Close (unsaved changes)' : 'Close'}
              aria-label={`Close ${tabLabel(buffer.path)}`}
              onClick={(event) => {
                event.stopPropagation();
                onClose(buffer.path);
              }}
              className="grid h-4 w-4 shrink-0 place-items-center rounded-[3px] text-vs-text hover:bg-white/15"
            >
              {dirty ? (
                <>
                  <span className="h-2 w-2 rounded-full bg-current group-hover/tab:hidden" />
                  <span className="hidden group-hover/tab:block">
                    <Icon name="close" size={13} />
                  </span>
                </>
              ) : (
                <span className={isActive ? '' : 'opacity-0 group-hover/tab:opacity-100'}>
                  <Icon name="close" size={13} />
                </span>
              )}
            </button>
          </div>
        );
      })}
    </div>
  );
}
