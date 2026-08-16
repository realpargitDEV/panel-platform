/**
 * The 24px strip along the bottom.
 *
 * Everything on it is measured. There is no room here for a number that might
 * be a placeholder, because a status bar is read at a glance and a glance has
 * no way to ask whether the figure is real — so a reading that is not yet
 * available is absent rather than zero.
 */
import type { PowerStatus, MachineLoad } from '../api';
import { formatBytes } from '../lib/format';
import { powerLook } from '../lib/power';

export default function ShellStatusBar({
  runningCount,
  load,
  power,
  onOpenProcesses,
  onOpenResources,
}: {
  runningCount: number;
  load: MachineLoad | null;
  power: PowerStatus | null;
  onOpenProcesses: () => void;
  onOpenResources: () => void;
}) {
  const look = powerLook(power);
  const measured = load !== null && load.measured;

  return (
    <footer
      className="flex shrink-0 items-center gap-3 border-t border-edge bg-surface px-2 text-[11px] text-muted"
      style={{ height: 'var(--h-statusbar)' }}
    >
      <button
        type="button"
        onClick={onOpenProcesses}
        className="flex items-center gap-1.5 rounded-[3px] px-1 hover:bg-raised hover:text-ink"
      >
        <span
          aria-hidden
          className={`h-[6px] w-[6px] rounded-full ${runningCount > 0 ? 'bg-ok' : 'bg-faint'}`}
        />
        {runningCount} running
      </button>

      <button
        type="button"
        onClick={onOpenResources}
        className="flex items-center gap-3 rounded-[3px] px-1 hover:bg-raised hover:text-ink"
      >
        <span className="tabular">
          CPU {load?.cpuPercent === null || load === null ? '—' : `${Math.round(load.cpuPercent)}%`}
        </span>
        <span className="tabular">
          RAM{' '}
          {measured
            ? formatBytes(load.totalMemoryBytes - load.availableMemoryBytes)
            : '—'}
        </span>
      </button>

      <span className="flex-1" />

      <span className="truncate px-1" title={look.summary}>
        {look.label}
      </span>
    </footer>
  );
}
