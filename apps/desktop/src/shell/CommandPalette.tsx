/**
 * The application's command palette.
 *
 * One search for the whole product: projects by name, and commands by name,
 * ranked together. Everything in it does something — the list is built from the
 * shell's own callbacks, so a command cannot exist here without an
 * implementation behind it.
 *
 * The ranking is the same subsequence match the editor's palette uses, imported
 * rather than reimplemented so both behave identically.
 */
import { useEffect, useMemo, useRef, useState } from 'react';

import type { ProjectSummary } from '../api';
import { matchCommands, type Command } from '../workspace/commands';
import Icon from '../ui/Icon';
import { Badge } from '../ui/primitives';
import { statusLook } from '../lib/projects';

export default function CommandPalette({
  commands,
  projects,
  onOpenProject,
  onClose,
}: {
  commands: Command[];
  projects: ProjectSummary[];
  onOpenProject: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement | null>(null);

  const projectMatches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const matching =
      needle.length === 0
        ? projects
        : projects.filter(
            (project) =>
              project.displayName.toLowerCase().includes(needle) ||
              project.slug.toLowerCase().includes(needle),
          );
    return matching.slice(0, 6);
  }, [projects, query]);

  const commandMatches = useMemo(
    () => matchCommands(commands, query).slice(0, 12),
    [commands, query],
  );

  // One flat list, so the arrow keys walk from the last project into the first
  // command without the user having to know there are two sections.
  const rows = useMemo(
    () => [
      ...projectMatches.map((project) => ({ kind: 'project' as const, project })),
      ...commandMatches.map((match) => ({ kind: 'command' as const, command: match.item })),
    ],
    [projectMatches, commandMatches],
  );

  useEffect(() => setIndex(0), [query]);

  useEffect(() => {
    list.current
      ?.querySelector<HTMLElement>(`[data-index="${index}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [index]);

  function accept(at: number) {
    const row = rows[at];
    if (!row) return;
    if (row.kind === 'project') {
      onClose();
      onOpenProject(row.project.id);
      return;
    }
    if (row.command.enabled === false) return;
    onClose();
    row.command.run();
  }

  function onKeyDown(event: React.KeyboardEvent) {
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        onClose();
        break;
      case 'ArrowDown':
        event.preventDefault();
        setIndex((current) => (rows.length === 0 ? 0 : (current + 1) % rows.length));
        break;
      case 'ArrowUp':
        event.preventDefault();
        setIndex((current) => (rows.length === 0 ? 0 : (current - 1 + rows.length) % rows.length));
        break;
      case 'Enter':
        event.preventDefault();
        accept(index);
        break;
    }
  }

  const firstCommandRow = projectMatches.length;

  return (
    <div
      className="fixed inset-0 z-[75] flex justify-center bg-black/50 px-4 pt-[12vh]"
      onMouseDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
        className="flex max-h-[65vh] w-full max-w-[600px] flex-col overflow-hidden rounded-[14px] border border-edge bg-overlay shadow-[0_24px_64px_rgba(0,0,0,0.6)]"
      >
        <div className="flex shrink-0 items-center gap-2.5 border-b border-edge px-4">
          <Icon name="search" size={16} className="text-muted" />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Search projects and commands"
            aria-label="Search projects and commands"
            className="h-12 flex-1 bg-transparent text-[14px] text-ink placeholder:text-faint select-text focus:outline-none"
          />
        </div>

        <div ref={list} className="min-h-0 flex-1 overflow-y-auto p-2">
          {rows.length === 0 && (
            <p className="px-2 py-6 text-center text-[13px] text-faint">
              Nothing matches “{query.trim()}”.
            </p>
          )}

          {projectMatches.length > 0 && <SectionLabel>Projects</SectionLabel>}

          {rows.map((row, at) => {
            const highlighted = at === index;
            const label = at === firstCommandRow && commandMatches.length > 0;

            return (
              <div key={row.kind === 'project' ? row.project.id : row.command.id}>
                {label && <SectionLabel>Commands</SectionLabel>}
                <button
                  type="button"
                  data-index={at}
                  onMouseMove={() => setIndex(at)}
                  onClick={() => accept(at)}
                  disabled={row.kind === 'command' && row.command.enabled === false}
                  title={row.kind === 'command' ? row.command.reason : undefined}
                  className={`flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-2 text-left disabled:opacity-40 ${
                    highlighted ? 'bg-raised' : ''
                  }`}
                >
                  {row.kind === 'project' ? (
                    <>
                      <Icon name="projects" size={15} className="text-muted" />
                      <span className="min-w-0 flex-1 truncate text-[13px] text-ink">
                        {row.project.displayName}
                      </span>
                      <Badge tone={statusLook(row.project.status).tone} dot>
                        {statusLook(row.project.status).label}
                      </Badge>
                    </>
                  ) : (
                    <>
                      <Icon name="command" size={15} className="text-muted" />
                      <span className="shrink-0 text-[13px] text-muted">
                        {row.command.category}:
                      </span>
                      <span className="min-w-0 flex-1 truncate text-[13px] text-ink">
                        {row.command.title}
                      </span>
                      {row.command.keybinding && (
                        <kbd className="shrink-0 rounded-[4px] border border-edge px-1.5 py-0.5 text-[11px] text-faint">
                          {row.command.keybinding}
                        </kbd>
                      )}
                    </>
                  )}
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return <p className="px-2.5 pt-2 pb-1 text-[11px] font-medium text-faint">{children}</p>;
}
