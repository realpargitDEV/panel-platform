import type { ProjectSummary } from '../api';

/**
 * The console screen for one project.
 *
 * Modelled on the panel's server view: a wide log pane with a toolbar and a
 * command box, an information panel down the right, and resource cards along
 * the bottom.
 *
 * Nothing here invents numbers. Log streaming and metrics collection are not
 * built, so those areas say so rather than showing a convincing zero — a
 * dashboard reporting 0% CPU for a project nobody is measuring is worse than
 * one admitting it does not know.
 */
export default function ProjectConsole({
  project,
  onBack,
}: {
  project: ProjectSummary;
  onBack: () => void;
}) {
  return (
    <div className="px-8 py-7">
      <button
        type="button"
        onClick={onBack}
        className="text-sm text-neutral-400 hover:text-neutral-200"
      >
        ← Projects
      </button>

      <p className="mt-4 text-xs font-semibold tracking-wider text-accent uppercase">Console</p>
      <h1 className="mt-1 text-3xl font-bold tracking-tight">{project.displayName}</h1>

      <div className="mt-6 grid gap-4 lg:grid-cols-[1fr_320px]">
        <section className="overflow-hidden rounded-xl border border-edge bg-surface">
          <div className="flex flex-wrap gap-2 border-b border-edge px-4 py-3">
            {['Copy all', 'Download', 'Hide timestamps', 'Clear'].map((label) => (
              <button
                key={label}
                type="button"
                disabled
                title="Log streaming is not built yet"
                className="rounded-md border border-edge bg-raised px-3 py-1.5 text-xs text-neutral-300 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {label}
              </button>
            ))}
          </div>

          <div className="h-80 overflow-y-auto bg-black/40 p-4 font-mono text-xs leading-relaxed text-neutral-400">
            <p>[--:--:--] Log streaming is not implemented yet.</p>
            <p>[--:--:--] This project is {project.status.toLowerCase()}.</p>
          </div>

          <div className="flex gap-2 border-t border-edge p-3">
            <input
              disabled
              placeholder="Type a command…"
              className="flex-1 rounded-md border border-edge bg-black/30 px-3 py-2 font-mono text-sm outline-none select-text disabled:cursor-not-allowed disabled:opacity-50"
            />
            <button
              type="button"
              disabled
              title="Sending commands to a running project is not built yet"
              className="rounded-md bg-accent px-4 py-2 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-50"
            >
              Send
            </button>
          </div>
        </section>

        <aside>
          <p className="mb-2 px-1 text-[11px] font-semibold tracking-wider text-neutral-500 uppercase">
            Project info
          </p>
          <div className="space-y-1.5">
            <InfoRow icon="▣" label="Status" value={project.status.toLowerCase()} />
            <InfoRow icon="◈" label="Wanted" value={project.desiredState.toLowerCase()} />
            <InfoRow icon="⌘" label="Slug" value={project.slug} />
            <InfoRow icon="▤" label="Type" value={project.projectType.toLowerCase()} />
            <InfoRow icon="◷" label="Uptime" value="not measured" muted />
            <InfoRow icon="◐" label="Memory" value="not measured" muted />
          </div>
        </aside>
      </div>

      <div className="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <ResourceCard icon="◉" tint="bg-emerald-500" title="CPU" caption="Processing power" />
        <ResourceCard icon="◐" tint="bg-accent" title="Memory" caption="Current usage" />
        <ResourceCard icon="⇅" tint="bg-purple-500" title="Network" caption="Data transfer" />
        <ResourceCard icon="▤" tint="bg-orange-500" title="Disk" caption="Storage space" />
      </div>
    </div>
  );
}

function InfoRow({
  icon,
  label,
  value,
  muted,
}: {
  icon: string;
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-edge bg-raised px-3 py-2.5">
      <span aria-hidden className="w-4 text-center text-xs text-neutral-500">
        {icon}
      </span>
      <span className="flex-1 text-sm text-neutral-400">{label}</span>
      <span className={`text-sm font-medium ${muted ? 'text-neutral-600' : 'text-neutral-100'}`}>
        {value}
      </span>
    </div>
  );
}

function ResourceCard({
  icon,
  tint,
  title,
  caption,
}: {
  icon: string;
  tint: string;
  title: string;
  caption: string;
}) {
  return (
    <div className="card-hover flex items-center gap-3 rounded-xl border border-edge bg-surface px-4 py-4">
      <span
        aria-hidden
        className={`grid h-10 w-10 shrink-0 place-items-center rounded-xl ${tint} text-sm shadow-lg`}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block leading-tight font-semibold">{title}</span>
        <span className="block text-[10px] font-semibold tracking-wider text-neutral-500 uppercase">
          {caption}
        </span>
      </span>
      {/* Deliberately not a number. Metrics collection is Phase 6. */}
      <span className="text-lg font-semibold text-neutral-600">—</span>
    </div>
  );
}
