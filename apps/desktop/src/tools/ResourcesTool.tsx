/**
 * What the machine has, and what is left of it.
 *
 * Compact by design. A resource panel that grows into a monitoring dashboard
 * stops being glanceable, and this one exists to answer "can I start another
 * one" in about a second.
 *
 * Every figure here is measured or absent. Nothing is estimated, and nothing is
 * drawn as zero because it has not been read yet.
 */
import { useCallback, useEffect, useState } from 'react';

import { errorMessage, machineLoad, type MachineLoad, type PowerStatus } from '../api';
import { formatBytes } from '../lib/format';
import { batteryPhrase, powerLook, temperaturePhrase } from '../lib/power';
import { ToolAction, ToolBody, ToolFact, ToolHeader, ToolSection } from './ToolChrome';

const POLL_MS = 2000;

export default function ResourcesTool({ power }: { power: PowerStatus | null }) {
  const [load, setLoad] = useState<MachineLoad | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const read = useCallback(() => {
    machineLoad()
      .then((next) => {
        setLoad(next);
        setFailure(null);
      })
      .catch((error: unknown) => setFailure(errorMessage(error)));
  }, []);

  useEffect(() => {
    read();
    const timer = setInterval(read, POLL_MS);
    return () => clearInterval(timer);
  }, [read]);

  const measured = load !== null && load.measured;
  const usedBytes = measured ? load.totalMemoryBytes - load.availableMemoryBytes : 0;
  const usedFraction = measured ? usedBytes / Math.max(1, load.totalMemoryBytes) : 0;

  const look = powerLook(power);
  const battery = batteryPhrase(power);
  const temperature = temperaturePhrase(power);

  return (
    <>
      <ToolHeader
        title="Resources"
        actions={<ToolAction icon="refresh" label="Refresh" onClick={read} />}
      />

      <ToolBody>
        {failure !== null && <p className="px-2.5 py-2 text-[12px] text-danger">{failure}</p>}

        <ToolSection label="Machine">
          <div className="px-2.5 pt-1 pb-2">
            <Meter
              label="Memory"
              value={
                measured ? `${formatBytes(usedBytes)} / ${formatBytes(load.totalMemoryBytes)}` : '—'
              }
              fraction={usedFraction}
              known={measured}
            />
            <Meter
              label="Processor"
              value={
                load?.cpuPercent === null || load === null ? '—' : `${Math.round(load.cpuPercent)}%`
              }
              fraction={load?.cpuPercent === null || load === null ? 0 : load.cpuPercent / 100}
              known={load?.cpuPercent !== null && load !== null}
            />
          </div>

          <ToolFact label="Cores" value={load === null ? '—' : String(load.logicalCores)} />
          {/* Named rather than folded into headroom: a user who sees free
              memory and is refused a start deserves to know what took it. */}
          <ToolFact label="Held back" value={measured ? formatBytes(load.reserveBytes) : '—'} />
          <ToolFact label="Headroom" value={measured ? formatBytes(load.headroomBytes) : '—'} />
        </ToolSection>

        <ToolSection label="Power">
          <ToolFact label="Profile" value={look.label} />
          <ToolFact label="Holding sleep" value={power?.sleepHeld === true ? 'yes' : 'no'} />
          {/* Absent rather than zero on a machine with no readable sensor,
              which is most Windows desktops. */}
          <ToolFact label="Temperature" value={temperature ?? 'no sensor'} />
          <ToolFact label="Battery" value={battery ?? 'no battery'} />
        </ToolSection>

        {power !== null && power.measured && (
          <p className="px-2.5 py-2 text-[11.5px] leading-relaxed text-faint">{power.reason}</p>
        )}
      </ToolBody>
    </>
  );
}

/**
 * A labelled bar.
 *
 * The bar is 3px and the number is the point. An unmeasured meter draws no fill
 * at all rather than an empty track that reads as "zero used".
 */
function Meter({
  label,
  value,
  fraction,
  known,
}: {
  label: string;
  value: string;
  fraction: number;
  known: boolean;
}) {
  const percent = Math.max(0, Math.min(100, fraction * 100));
  return (
    <div className="mb-2 last:mb-0">
      <div className="flex items-baseline justify-between gap-2 text-[12px]">
        <span className="text-muted">{label}</span>
        <span className="truncate text-ink tabular">{value}</span>
      </div>
      <div className="mt-1 h-[3px] w-full overflow-hidden rounded-full bg-edge">
        {known && (
          <div
            className={`h-full rounded-full ${percent > 90 ? 'bg-warn' : 'bg-accent'}`}
            style={{ width: `${percent}%` }}
          />
        )}
      </div>
    </div>
  );
}
