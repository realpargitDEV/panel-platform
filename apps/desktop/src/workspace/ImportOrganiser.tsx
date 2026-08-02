/**
 * The import, laid out so it can be rearranged before it happens.
 *
 * Shown when a drop is not obvious: several projects, a project mixed with
 * loose files, or anything the detection was unsure about. A single ordinary
 * folder never gets here — it takes the fast path — and a single clear project
 * gets the compact confirmation instead.
 *
 * Everything on this screen edits the *plan*. No file on this machine is
 * touched by any of it; the plan is executed only when Import is pressed.
 */
import { useMemo, useState } from 'react';

import Icon from './Icon';
import {
  conflictsWith,
  createGroup,
  mergeGroups,
  moveEntries,
  planFrom,
  removeEntries,
  renameGroup,
  resetGroup,
  setDestination,
  setInclude,
  setKind,
  setLayout,
  summarise,
  type GroupKind,
  type ImportGroup,
} from './importGroups';
import type { ImportCandidate } from '../api';
import {
  addRangeToSelection,
  emptySelection,
  extendSelection,
  isSelected,
  selectOnly,
  toggleSelection,
  type Selection,
  type VisibleEntry,
} from './selection';
import SelectionSurface from './SelectionSurface';

const KIND_LABEL: Record<GroupKind, string> = {
  project: 'Project',
  folder: 'Folder',
  files: 'Standalone files',
};

export default function ImportOrganiser({
  groups,
  candidates,
  destination,
  existingPaths,
  onChange,
  onImport,
  onCancel,
}: {
  groups: ImportGroup[];
  /** The original detection, so a group can be reset to it. */
  candidates: ImportCandidate[];
  destination: string;
  /** What is already in the project, for the conflict warning. */
  existingPaths: string[];
  onChange: (groups: ImportGroup[]) => void;
  onImport: () => void;
  onCancel: () => void;
}) {
  const [selection, setSelection] = useState<Selection>(emptySelection);
  const [renaming, setRenaming] = useState<string | null>(null);

  const plan = useMemo(() => planFrom(groups), [groups]);
  const conflicts = useMemo(() => conflictsWith(plan, existingPaths), [plan, existingPaths]);

  /** Every entry across every group, in the order they are drawn. */
  const visible = useMemo<VisibleEntry[]>(
    () =>
      groups.flatMap((group) =>
        group.entries.map((entry) => ({ path: entry.path, isDirectory: entry.isDirectory })),
      ),
    [groups],
  );

  /** Which group an entry currently belongs to. */
  const groupOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const group of groups) {
      for (const entry of group.entries) map.set(entry.path, group.id);
    }
    return map;
  }, [groups]);

  function onEntryPointerDown(path: string, event: React.MouseEvent) {
    if (event.shiftKey && (event.ctrlKey || event.metaKey)) {
      setSelection((current) => addRangeToSelection(current, path, visible));
    } else if (event.ctrlKey || event.metaKey) {
      setSelection((current) => toggleSelection(current, path));
    } else if (event.shiftKey) {
      setSelection((current) => extendSelection(current, path, visible));
    } else if (!isSelected(selection, path)) {
      setSelection(selectOnly(path));
    }
  }

  function dropInto(targetId: string, event: React.DragEvent) {
    event.preventDefault();
    const raw = event.dataTransfer.getData('application/x-import-entry');
    if (!raw) return;
    try {
      const paths: unknown = JSON.parse(raw);
      if (!Array.isArray(paths)) return;
      const moving = paths.filter((path): path is string => typeof path === 'string');
      if (moving.length === 0) return;

      // A selection can span several groups, and `moveEntries` moves out of one
      // at a time. Taking only the first entry's group would silently drop the
      // rest of the selection on the floor — the drag would look like it worked
      // and half the files would stay where they were.
      const bySource = new Map<string, string[]>();
      for (const path of moving) {
        const fromId = groupOf.get(path);
        if (fromId === undefined || fromId === targetId) continue;
        bySource.set(fromId, [...(bySource.get(fromId) ?? []), path]);
      }
      if (bySource.size === 0) return;

      let next = groups;
      for (const [fromId, entries] of bySource) {
        next = moveEntries(next, fromId, targetId, entries);
      }
      onChange(next);
    } catch {
      // A drag this build does not understand. Ignoring beats guessing.
    }
  }

  const included = groups.filter((group) => group.include && group.entries.length > 0);

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-6"
      onMouseDown={onCancel}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Organise the import"
        onMouseDown={(event) => event.stopPropagation()}
        className="flex max-h-[86vh] w-[min(860px,94vw)] flex-col border border-vs-border bg-[#12182a] shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
      >
        <div className="shrink-0 border-b border-vs-border px-4 py-3">
          <h2 className="text-[14px] font-semibold text-vs-text">Organise the import</h2>
          <p className="mt-0.5 text-[12px] text-vs-dim">
            {summarise(groups)} Nothing on your computer is changed by anything here.
          </p>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {groups.length === 0 && (
            <p className="py-6 text-center text-[13px] text-vs-dim">
              Everything has been removed from this import.
            </p>
          )}

          <SelectionSurface
            className="space-y-2"
            currentPaths={selection.selected}
            onSelectPaths={(paths) =>
              setSelection(paths.length === 0 ? emptySelection : { ...selection, selected: paths })
            }
            onClearSelection={() => setSelection(emptySelection)}
          >
            {groups.map((group) => (
              <section
                key={group.id}
                onDragOver={(event) => {
                  if (event.dataTransfer.types.includes('application/x-import-entry')) {
                    event.preventDefault();
                  }
                }}
                onDrop={(event) => dropInto(group.id, event)}
                className={`border ${
                  group.include ? 'border-vs-border' : 'border-vs-border/50 opacity-50'
                } bg-vs-editor`}
              >
                <header className="flex flex-wrap items-center gap-2 border-b border-vs-border px-2.5 py-2">
                  <input
                    type="checkbox"
                    checked={group.include}
                    aria-label={`Include ${group.name}`}
                    onChange={(event) =>
                      onChange(setInclude(groups, group.id, event.target.checked))
                    }
                    className="accent-[#3b82f6]"
                  />

                  {renaming === group.id ? (
                    <input
                      autoFocus
                      defaultValue={group.name}
                      aria-label="Group name"
                      onBlur={(event) => {
                        onChange(
                          renameGroup(groups, group.id, event.target.value.trim() || group.name),
                        );
                        setRenaming(null);
                      }}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') event.currentTarget.blur();
                        if (event.key === 'Escape') setRenaming(null);
                      }}
                      className="h-6 min-w-0 flex-1 border border-accent bg-vs-editor px-1.5 text-[13px] text-vs-text outline-none select-text"
                    />
                  ) : (
                    <button
                      type="button"
                      onDoubleClick={() => setRenaming(group.id)}
                      title="Double-click to rename"
                      className="min-w-0 flex-1 truncate text-left text-[13px] font-medium text-vs-text"
                    >
                      {group.name}
                    </button>
                  )}

                  <span className="shrink-0 rounded-full bg-white/5 px-2 py-0.5 text-[11px] text-vs-dim">
                    {KIND_LABEL[group.kind]}
                    {group.detection?.ecosystem ? ` · ${group.detection.ecosystem}` : ''}
                    {group.detection?.isMonorepo ? ' · workspace' : ''}
                  </span>
                  {!group.auto && (
                    <button
                      type="button"
                      onClick={() =>
                        onChange(resetGroup(groups, group.id, candidates, destination))
                      }
                      className="shrink-0 text-[11px] text-accent hover:underline"
                    >
                      Reset
                    </button>
                  )}
                </header>

                {group.detection && (
                  <p className="border-b border-vs-border/60 px-2.5 py-1.5 text-[11px] text-vs-dim">
                    {group.detection.explanation}
                    {group.detection.members.length > 0 && (
                      <> Members kept inside it: {group.detection.members.join(', ')}.</>
                    )}
                  </p>
                )}

                <div className="flex flex-wrap items-center gap-2 px-2.5 py-2 text-[12px]">
                  {group.kind !== 'files' && (
                    <label className="flex items-center gap-1.5 text-vs-dim">
                      Layout
                      <select
                        value={group.layout}
                        onChange={(event) =>
                          onChange(
                            setLayout(
                              groups,
                              group.id,
                              event.target.value === 'unwrap' ? 'unwrap' : 'keep',
                            ),
                          )
                        }
                        className="h-6 border border-vs-border bg-vs-editor px-1 text-[12px] text-vs-text outline-none"
                      >
                        <option value="unwrap">Contents into destination</option>
                        <option value="keep">Keep the folder</option>
                      </select>
                    </label>
                  )}

                  <label className="flex items-center gap-1.5 text-vs-dim">
                    Kind
                    <select
                      value={group.kind}
                      onChange={(event) =>
                        onChange(setKind(groups, group.id, event.target.value as GroupKind))
                      }
                      className="h-6 border border-vs-border bg-vs-editor px-1 text-[12px] text-vs-text outline-none"
                    >
                      <option value="project">Project</option>
                      <option value="folder">Folder</option>
                      <option value="files">Files</option>
                    </select>
                  </label>

                  <label className="flex min-w-0 flex-1 items-center gap-1.5 text-vs-dim">
                    Into
                    <input
                      value={group.destination}
                      placeholder="project root"
                      onChange={(event) =>
                        onChange(setDestination(groups, group.id, event.target.value.trim()))
                      }
                      className="h-6 min-w-0 flex-1 border border-vs-border bg-vs-editor px-1.5 font-mono text-[12px] text-vs-text outline-none select-text"
                    />
                  </label>
                </div>

                <ul className="px-1 pb-1.5">
                  {group.entries.map((entry) => (
                    <li key={entry.path}>
                      <div
                        data-row
                        data-path={entry.path}
                        draggable
                        onDragStart={(event) => {
                          const paths = isSelected(selection, entry.path)
                            ? selection.selected
                            : [entry.path];
                          event.dataTransfer.setData(
                            'application/x-import-entry',
                            JSON.stringify(paths),
                          );
                          event.dataTransfer.effectAllowed = 'move';
                        }}
                        onMouseDown={(event) => onEntryPointerDown(entry.path, event)}
                        className={`flex h-[22px] cursor-grab items-center gap-1.5 px-1.5 select-none ${
                          isSelected(selection, entry.path)
                            ? 'bg-vs-active text-white'
                            : 'text-vs-text hover:bg-white/5'
                        }`}
                      >
                        <span className="shrink-0 text-vs-dim">
                          <Icon name={entry.isDirectory ? 'folder' : 'file'} size={14} />
                        </span>
                        <span className="min-w-0 flex-1 truncate font-mono text-[12px]">
                          {entry.name}
                          {entry.isDirectory ? '/' : ''}
                        </span>
                        <button
                          type="button"
                          title="Remove from the import"
                          aria-label={`Remove ${entry.name} from the import`}
                          onClick={() => onChange(removeEntries(groups, [entry.path]))}
                          className="shrink-0 text-vs-dim hover:text-red-300"
                        >
                          <Icon name="close" size={12} />
                        </button>
                      </div>
                    </li>
                  ))}
                  {group.entries.length === 0 && (
                    <li className="px-1.5 py-2 text-[11px] text-vs-dim">
                      Empty — drag entries here.
                    </li>
                  )}
                </ul>
              </section>
            ))}
          </SelectionSurface>
        </div>

        {conflicts.length > 0 && (
          <div className="shrink-0 border-t border-amber-900/60 bg-amber-950/30 px-4 py-2">
            <p className="text-[12px] text-amber-300">
              {conflicts.length} destination{conflicts.length === 1 ? '' : 's'} already exist:{' '}
              <span className="font-mono">{conflicts.slice(0, 4).join(', ')}</span>
              {conflicts.length > 4 ? '…' : ''}. Folders merge; a file that already exists will stop
              its batch rather than be overwritten.
            </p>
          </div>
        )}

        <div className="flex shrink-0 flex-wrap items-center gap-2 border-t border-vs-border px-4 py-2.5">
          <button
            type="button"
            onClick={() => onChange(createGroup(groups, `Group ${groups.length + 1}`, 'folder'))}
            className="rounded-[2px] border border-vs-border px-2 py-1 text-[12px] text-vs-text hover:bg-white/5"
          >
            New group
          </button>
          <button
            type="button"
            disabled={included.length < 2}
            onClick={() =>
              onChange(
                mergeGroups(
                  groups,
                  included.map((group) => group.id),
                ),
              )
            }
            title="Combine every included group into the first"
            className="rounded-[2px] border border-vs-border px-2 py-1 text-[12px] text-vs-text hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Merge included
          </button>

          <span className="flex-1" />

          <button
            type="button"
            onClick={onCancel}
            className="rounded-[2px] border border-vs-border px-3 py-1 text-[13px] text-vs-text hover:bg-white/5"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={plan.batches.length === 0}
            onClick={onImport}
            className="rounded-[2px] bg-accent px-3 py-1 text-[13px] font-medium text-white hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Import
          </button>
        </div>
      </div>
    </div>
  );
}
