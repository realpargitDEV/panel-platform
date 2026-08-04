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
 *
 * The head of the rail is three bands with one divider under them: identity,
 * then the single primary action, then the destinations. Everything above the
 * divider says where you are and what you would create; everything below it
 * says where you can go. The gradients, the brand colour and the 170ms timing
 * are tokens in `styles.css` rather than values written here — see the
 * `sidebar` block there for why the blue is darker than `--color-accent`.
 */
import Icon, { type IconName } from '../ui/Icon';
import Logo from '../ui/Logo';
import { Menu, useMenu, type MenuItem } from '../ui/overlays';
import { IconButton } from '../ui/primitives';

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
  busyCount = 0,
  dockerAvailable,
  dockerSummary,
  onNavigate,
  onNewProject,
  onToggleCollapsed,
}: {
  view: View;
  collapsed: boolean;
  projectCount: number;
  /** Projects mid-transition. Drives the mark's pulse, so the motion means
   *  "something is happening" rather than decorating the header forever. */
  busyCount?: number;
  dockerAvailable: boolean;
  dockerSummary: string;
  onNavigate: (view: View) => void;
  onNewProject: () => void;
  onToggleCollapsed: () => void;
}) {
  const menu = useMenu();

  /** Both actions already exist on the rail; this is a second way to reach
   *  them from the header, not a new capability. */
  const workspaceItems: MenuItem[] = [
    {
      id: 'settings',
      label: 'Workspace settings',
      icon: 'settings',
      run: () => onNavigate('settings'),
    },
    {
      id: 'collapse',
      label: 'Collapse sidebar',
      icon: 'sidebar',
      run: onToggleCollapsed,
    },
  ];

  return (
    <nav
      aria-label="Main"
      className={`sidebar-surface flex shrink-0 flex-col transition-[width] duration-160 ${
        collapsed ? 'w-[60px]' : 'w-[228px]'
      }`}
    >
      {/* Identity. 64px gives two lines of text room to breathe against a 36px
          mark without the header becoming a band of its own. */}
      {/* One 12px gutter in both states, which is what keeps the mark on the
          same optical centre line as the icons below it when the rail
          collapses: 60px rail − 24px gutters = the mark's own 36px. */}
      <div className="flex h-16 items-center gap-2.5 px-3">
        <span
          className={`brand-tile grid h-9 w-9 shrink-0 place-items-center rounded-[10px] ${
            collapsed ? 'mx-auto' : ''
          }`}
        >
          <span
            className={busyCount > 0 ? 'animate-mark' : undefined}
            title={busyCount > 0 ? `${busyCount} project(s) changing state` : undefined}
          >
            <Logo size={22} title="Panel Platform" />
          </span>
        </span>

        {!collapsed && (
          <>
            {/* min-w-0 on both the column and its children: without it the
                flex item refuses to shrink and a long name pushes the menu
                button out of the rail instead of ellipsing. */}
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[13px] leading-4 font-semibold tracking-tight text-ink">
                Panel Platform
              </span>
              <span className="truncate text-[11px] leading-[14px] text-muted">
                Project workspace
              </span>
            </span>

            <IconButton icon="more" label="Workspace settings" size="sm" onClick={menu.open} />
          </>
        )}
      </div>

      {/* The one primary action. */}
      <div className="px-3 pb-3">
        <button
          type="button"
          onClick={onNewProject}
          title={collapsed ? 'New project' : undefined}
          aria-label={collapsed ? 'New project' : undefined}
          className={`btn-brand flex w-full items-center justify-center gap-2 rounded-[10px] text-[13px] font-semibold text-white ${
            collapsed ? 'h-9' : 'h-10'
          }`}
        >
          <Icon name="plus" size={collapsed ? 16 : 15} />
          {!collapsed && <span>New project</span>}
        </button>
      </div>

      {/* Separates identity and creation from navigation. Inset to the same
          12px gutter as everything above it, so it reads as a rule between
          two groups rather than a full-width cut across the rail. */}
      <div aria-hidden className="mx-3 h-px bg-edge" />

      <ul className={`flex-1 space-y-0.5 pt-3 ${collapsed ? 'px-2' : 'px-3'}`}>
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

      {menu.anchor && <Menu items={workspaceItems} anchor={menu.anchor} onClose={menu.close} />}
    </nav>
  );
}
