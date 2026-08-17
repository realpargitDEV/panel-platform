/**
 * Every project, with its state, in the sidebar.
 *
 * Running projects sort to the top. That is not a preference: when three things
 * are up and one of them is misbehaving, the list exists to find it, and
 * alphabetical order buries it among the twelve that are stopped.
 *
 * Choosing a project here points the window at it. It never starts or stops
 * anything, and it never stops what is already running.
 */
import { useState } from 'react';

import type { ProjectSummary } from '../api';
import { statusLook } from '../lib/projects';
import StatusDot from '../shell/StatusDot';
import { ToolAction, ToolBody, ToolEmpty, ToolHeader, ToolRow } from './ToolChrome';

export default function ProjectsTool({
  projects,
  currentId,
  onOpen,
  onNewProject,
  onRefresh,
}: {
  projects: ProjectSummary[] | null;
  currentId: string | null;
  onOpen: (id: string) => void;
  onNewProject: () => void;
  onRefresh: () => void;
}) {
  const [query, setQuery] = useState('');

  const all = projects ?? [];
  const needle = query.trim().toLowerCase();
  const matched =
    needle === ''
      ? all
      : all.filter(
          (project) =>
            project.displayName.toLowerCase().includes(needle) || project.slug.includes(needle),
        );

  const ordered = [...matched].sort((left, right) => {
    const rank = (project: ProjectSummary) => (project.status === 'RUNNING' ? 0 : 1);
    return rank(left) - rank(right) || left.displayName.localeCompare(right.displayName);
  });

  const running = ordered.filter((project) => project.status === 'RUNNING');
  const rest = ordered.filter((project) => project.status !== 'RUNNING');

  return (
    <>
      <ToolHeader
        title="Projects"
        actions={
          <>
            <ToolAction icon="refresh" label="Refresh" onClick={onRefresh} />
            <ToolAction icon="plus" label="New project" onClick={onNewProject} />
          </>
        }
      />

      {all.length > 6 && (
        <div className="shrink-0 border-b border-edge px-2 py-1.5">
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter projects…"
            className="h-[26px] w-full rounded-[5px] border border-edge bg-canvas px-2 text-[12px] text-ink select-text placeholder:text-faint focus:border-accent focus:outline-none"
          />
        </div>
      )}

      <ToolBody>
        {ordered.length === 0 ? (
          <ToolEmpty
            message={all.length === 0 ? 'No projects yet.' : 'Nothing matches that filter.'}
            action={all.length === 0 ? { label: 'New project', onClick: onNewProject } : undefined}
          />
        ) : (
          <>
            {running.length > 0 && (
              <Group label="Running" projects={running} currentId={currentId} onOpen={onOpen} />
            )}
            {rest.length > 0 && (
              <Group
                label={running.length > 0 ? 'Not running' : 'All projects'}
                projects={rest}
                currentId={currentId}
                onOpen={onOpen}
              />
            )}
          </>
        )}
      </ToolBody>
    </>
  );
}

function Group({
  label,
  projects,
  currentId,
  onOpen,
}: {
  label: string;
  projects: ProjectSummary[];
  currentId: string | null;
  onOpen: (id: string) => void;
}) {
  return (
    <div className="border-b border-edge/60 last:border-b-0">
      <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-semibold tracking-wide text-faint uppercase">
        {label} · {projects.length}
      </div>
      {projects.map((project) => {
        const look = statusLook(project.status);
        return (
          <ToolRow
            key={project.id}
            active={project.id === currentId}
            onClick={() => onOpen(project.id)}
            title={`${project.displayName} — ${look.label}`}
          >
            <StatusDot status={project.status} />
            <span className="min-w-0 flex-1 truncate">{project.displayName}</span>
            {/* The word, not only the dot. */}
            {look.transitioning && (
              <span className="shrink-0 text-[10.5px] text-faint">{look.label}</span>
            )}
          </ToolRow>
        );
      })}
    </div>
  );
}
