/**
 * The navigation rail.
 *
 * Collapsible down to icons, because the file editor wants the width back and
 * a person who knows the five destinations does not need them spelled out. The
 * collapsed state is remembered.
 *
 * The active item is marked with a 2px bar and a brighter label rather than a
 * filled block: a solid accent rectangle behind a nav item is the loudest thing
 * on the screen, and it is not the most important thing on the screen.
 */
import Icon, { type IconName } from '../ui/Icon';
import { Button } from '../ui/primitives';

export type View = 'overview' | 'projects' | 'activity' | 'discord' | 'settings';

const ITEMS: { id: View; label: string; icon: IconName }[] = [
  { id: 'overview', label: 'Overview', icon: 'overview' },
  { id: 'projects', label: 'Projects', icon: 'projects' },
  { id: 'activity', label: 'Activity', icon: 'activity' },
  { id: 'discord', label: 'Discord', icon: 'discord' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
];

export default function Sidebar({
  view,
  collapsed,
  projectCount,
  dockerAvailable,
  dockerSummary,
  onNavigate,
  onNewProject,
  onToggleCollapsed,
}: {
  view: View;
  collapsed: boolean;
  projectCount: number;
  dockerAvailable: boolean;
  dockerSummary: string;
  onNavigate: (view: View) => void;
  onNewProject: () => void;
  onToggleCollapsed: () => void;
}) {
  return (
    <nav
      aria-label="Main"
      className={`flex shrink-0 flex-col border-r border-edge bg-surface transition-[width] duration-160 ${
        collapsed ? 'w-[60px]' : 'w-[228px]'
      }`}
    >
      <div
        className={`flex h-14 items-center gap-2.5 ${collapsed ? 'justify-center px-2' : 'px-4'}`}
      >
        <span className="grid h-7 w-7 shrink-0 place-items-center rounded-[8px] bg-accent text-[13px] font-bold text-white">
          P
        </span>
        {!collapsed && (
          <span className="min-w-0 flex-1 truncate text-[14px] font-semibold tracking-tight">
            Panel Platform
          </span>
        )}
      </div>

      <div className={`pb-3 ${collapsed ? 'px-2' : 'px-3'}`}>
        {collapsed ? (
          <button
            type="button"
            onClick={onNewProject}
            title="New project"
            aria-label="New project"
            className="grid h-8 w-full place-items-center rounded-[8px] bg-accent text-white hover:bg-accent-hover"
          >
            <Icon name="plus" size={16} />
          </button>
        ) : (
          <Button variant="primary" icon="plus" full onClick={onNewProject}>
            New project
          </Button>
        )}
      </div>

      <ul className={`flex-1 space-y-0.5 ${collapsed ? 'px-2' : 'px-3'}`}>
        {ITEMS.map((item) => {
          const active = view === item.id;
          return (
            <li key={item.id}>
              <button
                type="button"
                onClick={() => onNavigate(item.id)}
                title={collapsed ? item.label : undefined}
                aria-current={active ? 'page' : undefined}
                className={`relative flex h-8 w-full items-center gap-2.5 rounded-[8px] text-[13px] ${
                  collapsed ? 'justify-center px-0' : 'px-2.5'
                } ${active ? 'bg-raised text-ink' : 'text-muted hover:bg-raised/60 hover:text-ink'}`}
              >
                {active && (
                  <span
                    aria-hidden
                    className="absolute top-1.5 -left-3 h-5 w-[2px] rounded-full bg-accent"
                  />
                )}
                <Icon name={item.icon} size={16} />
                {!collapsed && <span className="flex-1 truncate text-left">{item.label}</span>}
                {!collapsed && item.id === 'projects' && projectCount > 0 && (
                  <span className="tabular rounded-full bg-canvas px-1.5 text-[11px] text-muted">
                    {projectCount}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>

      <div className={`border-t border-edge py-2 ${collapsed ? 'px-2' : 'px-3'}`}>
        <button
          type="button"
          onClick={() => onNavigate('settings')}
          title={dockerSummary}
          className={`flex h-8 w-full items-center gap-2 rounded-[8px] text-[12px] text-muted hover:bg-raised hover:text-ink ${
            collapsed ? 'justify-center px-0' : 'px-2'
          }`}
        >
          <span
            aria-hidden
            className={`h-2 w-2 shrink-0 rounded-full ${dockerAvailable ? 'bg-ok' : 'bg-warn'}`}
          />
          {!collapsed && (
            <span className="min-w-0 flex-1 truncate text-left">
              {dockerAvailable ? 'Docker connected' : 'Docker unavailable'}
            </span>
          )}
        </button>

        <button
          type="button"
          onClick={onToggleCollapsed}
          title={collapsed ? 'Expand sidebar (Ctrl+B)' : 'Collapse sidebar (Ctrl+B)'}
          aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
          className={`mt-0.5 flex h-8 w-full items-center gap-2 rounded-[8px] text-[12px] text-muted hover:bg-raised hover:text-ink ${
            collapsed ? 'justify-center px-0' : 'px-2'
          }`}
        >
          <Icon name={collapsed ? 'chevron-right' : 'chevron-left'} size={16} />
          {!collapsed && <span className="flex-1 text-left">Collapse</span>}
        </button>
      </div>
    </nav>
  );
}
