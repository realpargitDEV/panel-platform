export type View = 'dashboard' | 'projects' | 'discord' | 'settings';

/** Nav items grouped under labelled sections, the way the panel does it. */
const GROUPS: {
  label: string;
  items: { id: View; label: string; icon: string; tint?: string }[];
}[] = [
  {
    label: 'Main',
    items: [{ id: 'dashboard', label: 'Dashboard', icon: '◈' }],
  },
  {
    label: 'Projects',
    items: [{ id: 'projects', label: 'Your projects', icon: '▤' }],
  },
  {
    label: 'Account',
    items: [
      { id: 'discord', label: 'Discord', icon: '◇' },
      { id: 'settings', label: 'Settings', icon: '⚙' },
    ],
  },
];

export default function Sidebar({
  view,
  onNavigate,
  projectCount,
  dockerAvailable,
  onNewProject,
}: {
  view: View;
  onNavigate: (view: View) => void;
  projectCount: number;
  dockerAvailable: boolean;
  onNewProject: () => void;
}) {
  return (
    <nav className="flex w-56 shrink-0 flex-col overflow-y-auto border-r border-edge bg-surface">
      <div className="px-4 py-5">
        <div className="flex items-center gap-2.5">
          <span className="grid h-9 w-9 place-items-center rounded-xl bg-accent text-sm font-bold">
            P
          </span>
          <span>
            <span className="block font-semibold leading-tight tracking-tight">Panel Platform</span>
            <span className="block text-xs text-neutral-500">Hosting on your own machine</span>
          </span>
        </div>
      </div>

      {GROUPS.map((group) => (
        <div key={group.label} className="px-3 pb-1">
          <p className="px-2 pb-1.5 pt-3 text-[11px] font-semibold uppercase tracking-wider text-neutral-500">
            {group.label}
          </p>
          <ul className="space-y-1.5">
            {group.items.map((item) => {
              const active = view === item.id;
              return (
                <li key={item.id}>
                  <button
                    type="button"
                    onClick={() => onNavigate(item.id)}
                    aria-current={active ? 'page' : undefined}
                    className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm transition-colors ${
                      active
                        ? 'bg-accent/15 text-white'
                        : 'bg-raised/60 text-neutral-400 hover:bg-raised hover:text-neutral-200'
                    }`}
                  >
                    <span
                      aria-hidden
                      className={`w-4 text-center ${active ? 'text-accent' : 'opacity-70'}`}
                    >
                      {item.icon}
                    </span>
                    <span className="flex-1">{item.label}</span>
                    {item.id === 'projects' && projectCount > 0 && (
                      <span className="rounded-full bg-white/10 px-2 py-0.5 text-xs">
                        {projectCount}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ))}

      {/* The panel gives creation its own coloured entry rather than burying it
          on the projects page. */}
      <div className="px-3 pt-1.5">
        <button
          type="button"
          onClick={onNewProject}
          className="flex w-full items-center gap-3 rounded-lg bg-raised/60 px-3 py-2.5 text-left text-sm text-neutral-300 hover:bg-raised"
        >
          <span aria-hidden className="w-4 text-center text-emerald-400">
            +
          </span>
          New project
        </button>
      </div>

      <div className="mt-auto px-4 py-4">
        <div className="flex items-center gap-2 rounded-lg bg-raised/60 px-3 py-2.5 text-xs text-neutral-400">
          <span
            className={`h-2 w-2 rounded-full ${dockerAvailable ? 'bg-emerald-500' : 'bg-amber-500'}`}
            aria-hidden
          />
          {dockerAvailable ? 'Docker connected' : 'Docker unavailable'}
        </div>
      </div>
    </nav>
  );
}
