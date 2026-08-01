/**
 * The 48px column of icons down the far left.
 *
 * Choosing the section that is already showing collapses the sidebar, and
 * choosing it again brings it back — the behaviour VS Code has, and the reason
 * the active item is a toggle rather than a radio.
 */
import Icon, { type IconName } from './Icon';
import type { ActivityView } from './layout';

interface Section {
  id: ActivityView;
  icon: IconName;
  label: string;
  /** A count drawn in the corner of the icon, e.g. unsaved files. */
  badge?: number;
}

export default function ActivityBar({
  view,
  visible,
  unsaved,
  onSelect,
  onProjects,
  onSettings,
}: {
  view: ActivityView;
  /** False while the sidebar is collapsed: nothing is highlighted then. */
  visible: boolean;
  /** How many open files have unsaved changes. */
  unsaved: number;
  onSelect: (view: ActivityView) => void;
  onProjects: () => void;
  onSettings: () => void;
}) {
  const sections: Section[] = [
    { id: 'explorer', icon: 'file', label: 'Explorer (Ctrl+Shift+E)', badge: unsaved },
    { id: 'search', icon: 'search', label: 'Search (Ctrl+Shift+F)' },
    { id: 'source-control', icon: 'source-control', label: 'Source Control' },
    { id: 'run', icon: 'run', label: 'Run and Debug' },
    { id: 'extensions', icon: 'extensions', label: 'Extensions' },
  ];

  return (
    <nav
      aria-label="Activity bar"
      className="flex w-12 shrink-0 flex-col items-center border-r border-vs-border bg-vs-activity"
    >
      {sections.map((section) => (
        <ActivityButton
          key={section.id}
          icon={section.icon}
          label={section.label}
          active={visible && view === section.id}
          badge={section.badge}
          onClick={() => onSelect(section.id)}
        />
      ))}

      <span className="flex-1" />

      <ActivityButton
        icon="folder"
        label="Back to your projects"
        active={false}
        onClick={onProjects}
      />
      <ActivityButton
        icon="account"
        label="Account"
        active={visible && view === 'account'}
        onClick={() => onSelect('account')}
      />
      <ActivityButton icon="settings" label="Settings" active={false} onClick={onSettings} />
    </nav>
  );
}

function ActivityButton({
  icon,
  label,
  active,
  badge,
  onClick,
}: {
  icon: IconName;
  label: string;
  active: boolean;
  badge?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={`relative grid h-12 w-12 place-items-center border-l-2 ${
        active ? 'border-accent text-white' : 'border-transparent text-vs-dim hover:text-vs-text'
      }`}
    >
      <Icon name={icon} size={22} />
      {badge !== undefined && badge > 0 && (
        <span className="absolute right-2 bottom-2 grid h-4 min-w-4 place-items-center rounded-full bg-vs-badge px-1 text-[10px] font-semibold text-white">
          {badge > 99 ? '99+' : badge}
        </span>
      )}
    </button>
  );
}
