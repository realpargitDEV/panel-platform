/**
 * The 48px column of tools down the far left.
 *
 * Every entry here opens something that reads real state. There is no Source
 * Control and no Extensions: this application does not manage a repository and
 * has no extension host, and a rail item that opens a panel explaining what the
 * product does not do is a worse answer than not offering the item.
 *
 * Choosing the tool that is already showing collapses the sidebar, and choosing
 * it again brings it back — so the active item is a toggle rather than a radio,
 * which is the behaviour anyone arriving from an editor expects.
 */
import Icon, { type IconName } from '../ui/Icon';
import type { ToolId } from './shellLayout';

interface Tool {
  id: ToolId;
  icon: IconName;
  label: string;
  /** Drawn in the corner of the icon. Omitted at zero rather than shown as 0. */
  badge?: number;
}

export default function ActivityRail({
  tool,
  visible,
  runningCount,
  onSelect,
}: {
  tool: ToolId;
  /** False while the sidebar is collapsed: nothing reads as open then. */
  visible: boolean;
  runningCount: number;
  onSelect: (tool: ToolId) => void;
}) {
  const top: Tool[] = [
    { id: 'projects', icon: 'projects', label: 'Projects' },
    { id: 'processes', icon: 'play', label: 'Processes', badge: runningCount },
    { id: 'console', icon: 'terminal', label: 'Console' },
    { id: 'ports', icon: 'network', label: 'Ports' },
    { id: 'environment', icon: 'shield', label: 'Environment' },
    { id: 'resources', icon: 'cpu', label: 'Resources' },
    { id: 'discord', icon: 'discord', label: 'Discord bots' },
  ];

  return (
    <nav
      aria-label="Tools"
      className="flex w-12 shrink-0 flex-col items-center border-r border-edge bg-canvas"
      style={{ width: 'var(--w-activitybar)' }}
    >
      {top.map((entry) => (
        <RailButton
          key={entry.id}
          entry={entry}
          active={visible && tool === entry.id}
          onClick={() => onSelect(entry.id)}
        />
      ))}

      <span className="flex-1" />

      <RailButton
        entry={{ id: 'settings', icon: 'settings', label: 'Settings' }}
        active={visible && tool === 'settings'}
        onClick={() => onSelect('settings')}
      />
    </nav>
  );
}

function RailButton({
  entry,
  active,
  onClick,
}: {
  entry: Tool;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={entry.label}
      aria-label={entry.label}
      aria-pressed={active}
      onClick={onClick}
      className={`relative grid h-10 w-12 shrink-0 place-items-center transition-colors duration-100 ${
        active ? 'text-ink' : 'text-faint hover:text-muted'
      }`}
    >
      {/* A 2px bar rather than a filled pill: the pill is the shape that makes
          a compact rail look like a phone's tab bar. */}
      {active && (
        <span aria-hidden className="absolute left-0 h-[18px] w-[2px] rounded-r-[2px] bg-accent" />
      )}
      <Icon name={entry.icon} size={18} />
      {entry.badge !== undefined && entry.badge > 0 && (
        <span
          aria-hidden
          className="absolute right-1.5 bottom-1.5 grid h-3.5 min-w-3.5 place-items-center rounded-full bg-accent px-1 text-[9px] font-semibold text-white tabular"
        >
          {entry.badge > 99 ? '99+' : entry.badge}
        </span>
      )}
    </button>
  );
}
