/**
 * The breadcrumb in the top bar, and the list it opens.
 *
 * Every project in the list carries its own state, because several can be
 * running at once and the switcher is where that becomes visible. Switching
 * projects here changes what the window is pointed at and nothing else: it
 * never stops what is already running, which is the single most important
 * thing about this control.
 */
import { useEffect, useRef, useState } from 'react';

import type { ProjectSummary } from '../api';
import { statusLook } from '../lib/projects';
import Icon from '../ui/Icon';
import StatusDot from './StatusDot';

export default function ProjectSwitcher({
  projects,
  current,
  onOpen,
  onBrowse,
}: {
  projects: ProjectSummary[] | null;
  current: ProjectSummary | null;
  onOpen: (id: string) => void;
  onBrowse: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const wrap = useRef<HTMLDivElement | null>(null);
  const input = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return undefined;
    input.current?.focus();
    function onDown(event: MouseEvent) {
      if (wrap.current !== null && !wrap.current.contains(event.target as Node)) setOpen(false);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape') setOpen(false);
    }
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const all = projects ?? [];
  const needle = query.trim().toLowerCase();
  const matches =
    needle === ''
      ? all
      : all.filter((p) => p.displayName.toLowerCase().includes(needle) || p.slug.includes(needle));

  // Running first: when several are up, they are what the list is for.
  const ordered = [...matches].sort((left, right) => {
    const rank = (project: ProjectSummary) => (project.status === 'RUNNING' ? 0 : 1);
    return rank(left) - rank(right) || left.displayName.localeCompare(right.displayName);
  });

  return (
    <div ref={wrap} className="relative flex min-w-0 items-center">
      <button
        type="button"
        onClick={() => {
          setQuery('');
          setOpen((value) => !value);
        }}
        aria-expanded={open}
        title={current === null ? 'Choose a project' : current.displayName}
        className="flex h-[26px] min-w-0 items-center gap-1.5 rounded-[5px] px-1.5 text-[12.5px] hover:bg-raised"
      >
        {current !== null && <StatusDot status={current.status} />}
        <span className="min-w-0 truncate text-ink">
          {current === null ? 'No project' : current.displayName}
        </span>
        <Icon name="chevron-down" size={11} />
      </button>

      {open && (
        <div className="absolute top-full left-0 z-50 mt-1 w-[280px] rounded-[7px] border border-edge-strong bg-overlay p-1 shadow-lg">
          <input
            ref={input}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search projects…"
            className="mb-1 h-[28px] w-full rounded-[5px] border border-edge bg-canvas px-2 text-[12.5px] text-ink select-text placeholder:text-faint focus:border-accent focus:outline-none"
          />

          <div className="max-h-[320px] overflow-y-auto">
            {ordered.length === 0 ? (
              <p className="px-2 py-3 text-center text-[12px] text-muted">
                {all.length === 0 ? 'No projects yet.' : 'Nothing matches.'}
              </p>
            ) : (
              ordered.map((project) => {
                const look = statusLook(project.status);
                return (
                  <button
                    key={project.id}
                    type="button"
                    onClick={() => {
                      setOpen(false);
                      onOpen(project.id);
                    }}
                    className={`flex h-[30px] w-full items-center gap-2 rounded-[5px] px-2 text-left text-[12.5px] hover:bg-raised ${
                      project.id === current?.id ? 'bg-raised text-ink' : 'text-muted'
                    }`}
                  >
                    <StatusDot status={project.status} />
                    <span className="min-w-0 flex-1 truncate text-ink">{project.displayName}</span>
                    {/* The word as well as the dot: colour alone is not a state. */}
                    <span className="shrink-0 text-[11px] text-faint">{look.label}</span>
                  </button>
                );
              })
            )}
          </div>

          <div className="mt-1 h-px bg-edge" aria-hidden />
          <button
            type="button"
            onClick={() => {
              setOpen(false);
              onBrowse();
            }}
            className="flex h-[30px] w-full items-center rounded-[5px] px-2 text-left text-[12.5px] text-muted hover:bg-raised hover:text-ink"
          >
            All projects
          </button>
        </div>
      )}
    </div>
  );
}
