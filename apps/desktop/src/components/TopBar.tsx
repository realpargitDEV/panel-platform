import type { SystemStatus } from '../api';

/** The stat chips and identity strip across the top of the panel. */
export default function TopBar({
  status,
  projectCount,
  runningCount,
}: {
  status: SystemStatus | null;
  projectCount: number;
  runningCount: number;
}) {
  return (
    <header className="flex items-center justify-end gap-3 border-b border-edge px-8 py-3">
      <Chip icon="▤" label="Projects" value={String(projectCount)} />
      <Chip icon="●" label="Running" value={String(runningCount)} />
      <span className="ml-2 flex items-center gap-2 text-sm text-neutral-300">
        <span className="grid h-7 w-7 place-items-center rounded-full bg-accent text-xs font-bold">
          P
        </span>
        {status ? `v${status.appVersion}` : '…'}
      </span>
    </header>
  );
}

function Chip({ icon, label, value }: { icon: string; label: string; value: string }) {
  return (
    <div className="flex items-center gap-2.5 rounded-lg border border-edge bg-raised px-3 py-1.5">
      <span
        aria-hidden
        className="grid h-7 w-7 place-items-center rounded-md bg-accent/15 text-xs text-accent"
      >
        {icon}
      </span>
      <span>
        <span className="block text-[10px] font-semibold uppercase tracking-wider text-neutral-500">
          {label}
        </span>
        <span className="block text-sm font-semibold tabular-nums leading-tight">{value}</span>
      </span>
    </div>
  );
}
