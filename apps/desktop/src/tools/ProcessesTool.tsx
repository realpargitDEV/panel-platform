/**
 * What this machine is carrying right now.
 *
 * The answer to "why are the fans on", and the place the multi-project story
 * becomes concrete: several projects up at once, each with its own cost, and
 * clicking one goes straight to it.
 *
 * A host project's memory is read from its process tree. A container's is its
 * declared limit, because the daemon's stats endpoint is not wired up — those
 * rows say `limit` rather than presenting a bound as a measurement.
 */
import { useCallback, useEffect, useState } from 'react';

import { errorMessage, machineLoad, type MachineLoad } from '../api';
import { formatBytes, formatDuration } from '../lib/format';
import { uptimeSeconds } from '../components/RunningPanel';
import { ToolAction, ToolBody, ToolEmpty, ToolHeader } from './ToolChrome';

/** Matches the core's own sampler, so a figure changes when it is resampled. */
const POLL_MS = 2000;

export default function ProcessesTool({
  currentId,
  onOpen,
}: {
  currentId: string | null;
  onOpen: (id: string) => void;
}) {
  const [load, setLoad] = useState<MachineLoad | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  /** Re-rendered on a timer so uptimes count up between polls. */
  const [, setTick] = useState(0);

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

  useEffect(() => {
    const timer = setInterval(() => setTick((value) => value + 1), 1000);
    return () => clearInterval(timer);
  }, []);

  const running = load?.running ?? [];

  return (
    <>
      <ToolHeader
        title={`Processes${running.length > 0 ? ` · ${running.length}` : ''}`}
        actions={<ToolAction icon="refresh" label="Refresh" onClick={read} />}
      />

      <ToolBody>
        {failure !== null && <p className="px-2.5 py-2 text-[12px] text-danger">{failure}</p>}

        {running.length === 0 ? (
          <ToolEmpty message="Nothing is running." />
        ) : (
          running.map((project) => {
            const up = uptimeSeconds(project.startedAt);
            return (
              <button
                key={project.projectId}
                type="button"
                onClick={() => onOpen(project.projectId)}
                title={project.displayName}
                className={`flex w-full flex-col gap-1 border-b border-edge/60 px-2.5 py-2 text-left last:border-b-0 hover:bg-raised/60 ${
                  project.projectId === currentId ? 'bg-raised' : ''
                }`}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <span aria-hidden className="h-[7px] w-[7px] shrink-0 rounded-full bg-ok" />
                  <span className="min-w-0 flex-1 truncate text-[12.5px] text-ink">
                    {project.displayName}
                  </span>
                  {project.port !== null && (
                    <span className="shrink-0 text-[11px] text-muted tabular">:{project.port}</span>
                  )}
                </div>

                <div className="flex items-center gap-3 pl-[15px] text-[11px] text-faint tabular">
                  <span title={project.measured ? 'Measured' : "The project's declared limit"}>
                    {formatBytes(project.memoryBytes)}
                    {!project.measured && <span className="ml-1 not-tabular">limit</span>}
                  </span>
                  {/* Absent rather than zero: a container's CPU is not read. */}
                  <span>{project.cpuPercent === null ? '—' : `${Math.round(project.cpuPercent)}%`}</span>
                  <span>{up === null ? '—' : formatDuration(up)}</span>
                </div>
              </button>
            );
          })
        )}
      </ToolBody>
    </>
  );
}
