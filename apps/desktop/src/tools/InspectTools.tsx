/**
 * Ports and environment, both read from the open project's own record.
 *
 * Together in one file because they are the same shape of tool — a list of the
 * project's declared configuration — and they load from the same call.
 *
 * Neither invents anything. A project with no ports shows no ports; a secret's
 * value is not held in the window at all until the user asks for it, and this
 * panel never writes one into a log.
 */
import { useCallback, useEffect, useState } from 'react';

import { errorMessage, projectDetails, type ProjectDetail, type ProjectSummary } from '../api';
import Icon from '../ui/Icon';
import { toast } from '../ui/toast';
import { ToolAction, ToolBody, ToolEmpty, ToolHeader, ToolRow } from './ToolChrome';

/** Shared loader: both tools need exactly the project's detail record. */
function useDetail(project: ProjectSummary | null) {
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const load = useCallback(() => {
    if (project === null) {
      setDetail(null);
      return;
    }
    projectDetails(project.id)
      .then((next) => {
        setDetail(next);
        setFailure(null);
      })
      .catch((error: unknown) => setFailure(errorMessage(error)));
  }, [project]);

  useEffect(load, [load, project?.status]);

  return { detail, failure, reload: load };
}

// ----------------------------------------------------------------- ports

export function PortsTool({
  project,
  onOpenProjects,
}: {
  project: ProjectSummary | null;
  onOpenProjects: () => void;
}) {
  const { detail, failure, reload } = useDetail(project);
  const running = project?.status === 'RUNNING';

  return (
    <>
      <ToolHeader
        title="Ports"
        actions={<ToolAction icon="refresh" label="Refresh" onClick={reload} />}
      />
      <ToolBody>
        {project === null ? (
          <ToolEmpty message="No project open." action={{ label: 'Choose a project', onClick: onOpenProjects }} />
        ) : failure !== null ? (
          <p className="px-2.5 py-2 text-[12px] text-danger">{failure}</p>
        ) : detail === null ? (
          <p className="px-2.5 py-2 text-[12px] text-muted">Loading…</p>
        ) : detail.ports.length === 0 ? (
          <ToolEmpty message="This project publishes no ports." />
        ) : (
          detail.ports.map((port) => {
            const number = port.hostPort ?? port.containerPort;
            const address = `localhost:${number}`;
            // Only offered while the project is up: a link to a port nothing is
            // listening on is a browser error page with this application's name
            // on it.
            return (
              <div
                key={`${port.containerPort}-${port.protocol}`}
                className="flex items-center gap-2 border-b border-edge/60 px-2.5 py-1.5 text-[12.5px] last:border-b-0"
              >
                <span className="w-12 shrink-0 text-ink tabular">{number}</span>
                <span className="w-10 shrink-0 text-[11px] text-faint uppercase">
                  {port.protocol}
                </span>
                <span className="min-w-0 flex-1 truncate text-muted">{address}</span>

                <button
                  type="button"
                  title="Copy address"
                  aria-label="Copy address"
                  onClick={() => {
                    void navigator.clipboard
                      .writeText(address)
                      .then(() => toast.success('Address copied'))
                      .catch(() => toast.error('Could not copy the address'));
                  }}
                  className="grid h-5 w-5 shrink-0 place-items-center rounded-[3px] text-faint hover:bg-raised hover:text-ink"
                >
                  <Icon name="copy" size={12} />
                </button>

                {running && port.protocol.toUpperCase() === 'TCP' && (
                  <a
                    href={`http://${address}`}
                    target="_blank"
                    rel="noreferrer"
                    title="Open in your browser"
                    className="grid h-5 w-5 shrink-0 place-items-center rounded-[3px] text-faint hover:bg-raised hover:text-ink"
                  >
                    <Icon name="external" size={12} />
                  </a>
                )}
              </div>
            );
          })
        )}
      </ToolBody>
    </>
  );
}

// ----------------------------------------------------------- environment

export function EnvironmentTool({
  project,
  onOpenProjects,
}: {
  project: ProjectSummary | null;
  onOpenProjects: () => void;
}) {
  const { detail, failure, reload } = useDetail(project);
  const [revealed, setRevealed] = useState<Set<string>>(new Set());

  // Anything revealed belongs to the project it was revealed on. Carrying the
  // set across a project switch would show one project's secret under
  // another's name.
  useEffect(() => setRevealed(new Set()), [project?.id]);

  return (
    <>
      <ToolHeader
        title="Environment"
        actions={<ToolAction icon="refresh" label="Refresh" onClick={reload} />}
      />
      <ToolBody>
        {project === null ? (
          <ToolEmpty message="No project open." action={{ label: 'Choose a project', onClick: onOpenProjects }} />
        ) : failure !== null ? (
          <p className="px-2.5 py-2 text-[12px] text-danger">{failure}</p>
        ) : detail === null ? (
          <p className="px-2.5 py-2 text-[12px] text-muted">Loading…</p>
        ) : detail.envVars.length === 0 ? (
          <ToolEmpty message="This project has no environment variables." />
        ) : (
          detail.envVars.map((variable) => {
            const shown = revealed.has(variable.key);
            return (
              <div
                key={variable.key}
                className="border-b border-edge/60 px-2.5 py-1.5 last:border-b-0"
              >
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate font-mono text-[12px] text-ink">
                    {variable.key}
                  </span>
                  {variable.restartRequired && (
                    <span
                      className="shrink-0 text-[10px] text-warn"
                      title="Changing this takes effect the next time the project starts"
                    >
                      restart
                    </span>
                  )}
                  {variable.isSecret && (
                    <button
                      type="button"
                      title={shown ? 'Hide' : 'Reveal'}
                      aria-label={shown ? 'Hide value' : 'Reveal value'}
                      onClick={() =>
                        setRevealed((previous) => {
                          const next = new Set(previous);
                          if (next.has(variable.key)) next.delete(variable.key);
                          else next.add(variable.key);
                          return next;
                        })
                      }
                      className="grid h-5 w-5 shrink-0 place-items-center rounded-[3px] text-faint hover:bg-raised hover:text-ink"
                    >
                      <Icon name={shown ? 'blocked' : 'shield'} size={12} />
                    </button>
                  )}
                </div>

                <p className="mt-0.5 truncate font-mono text-[11.5px] text-muted select-text">
                  {/* A secret's value is only ever what the core sent. When it
                      sent nothing, that is what is shown — never a guess. */}
                  {variable.isSecret && !shown
                    ? '••••••••••••'
                    : (variable.value ?? <span className="not-italic text-faint">not stored here</span>)}
                </p>
              </div>
            );
          })
        )}
      </ToolBody>
    </>
  );
}

/** Re-exported for the shell, which renders one of the two by tool id. */
export { ToolRow };
