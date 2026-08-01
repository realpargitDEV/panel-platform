/**
 * The activity log.
 *
 * Every line is an entry the core wrote to its audit log as it happened. There
 * is no synthesis here and no filler: an empty list means nothing has been done
 * yet, which is a true and useful thing to say.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';

import { errorMessage, recentActivity, type ActivityEntry, type ProjectSummary } from '../api';
import { formatRelative, formatTimestamp, parseTimestamp } from '../lib/format';
import { actionTone, describeAction } from '../lib/projects';
import Icon from '../ui/Icon';
import Select from '../ui/Select';
import {
  Badge,
  Button,
  Card,
  EmptyState,
  IconButton,
  PageShell,
  Skeleton,
  TextInput,
} from '../ui/primitives';

const PAGE_SIZE = 100;

type ResultFilter = 'all' | 'success' | 'failure';

export default function Activity({
  projects,
  onOpenProject,
}: {
  projects: ProjectSummary[] | null;
  onOpenProject: (id: string) => void;
}) {
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [result, setResult] = useState<ResultFilter>('all');
  const [projectId, setProjectId] = useState('all');

  const load = useCallback(() => {
    setFailure(null);
    recentActivity(PAGE_SIZE, projectId === 'all' ? undefined : projectId)
      .then(setEntries)
      .catch((error: unknown) => {
        setEntries([]);
        setFailure(errorMessage(error));
      });
  }, [projectId]);

  useEffect(load, [load]);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (entries ?? []).filter((entry) => {
      const matchesResult =
        result === 'all' ||
        (result === 'success' && actionTone(entry.result) === 'ok') ||
        (result === 'failure' && actionTone(entry.result) !== 'ok');
      if (!matchesResult) return false;
      if (needle.length === 0) return true;
      return (
        entry.action.toLowerCase().includes(needle) ||
        (entry.targetLabel ?? '').toLowerCase().includes(needle) ||
        describeAction(entry.action, entry.targetLabel).toLowerCase().includes(needle)
      );
    });
  }, [entries, query, result]);

  // Grouped by day, because a flat list of two hundred timestamps is a wall.
  const groups = useMemo(() => {
    const byDay = new Map<string, ActivityEntry[]>();
    for (const entry of visible) {
      const date = parseTimestamp(entry.occurredAt);
      const key = date ? date.toDateString() : 'Unknown';
      const bucket = byDay.get(key);
      if (bucket) bucket.push(entry);
      else byDay.set(key, [entry]);
    }
    return [...byDay.entries()];
  }, [visible]);

  return (
    <PageShell
      title="Activity"
      description="What Panel Platform has done, newest first."
      actions={<IconButton icon="refresh" label="Refresh" onClick={load} />}
    >
      <div className="mb-4 flex flex-wrap items-end gap-2">
        <div className="min-w-[200px] flex-1">
          <TextInput value={query} onChange={setQuery} placeholder="Search activity" />
        </div>
        <div className="w-[160px]">
          <Select<ResultFilter>
            value={result}
            onChange={setResult}
            options={[
              { value: 'all', label: 'Any result' },
              { value: 'success', label: 'Succeeded' },
              { value: 'failure', label: 'Failed or denied' },
            ]}
          />
        </div>
        <div className="w-[190px]">
          <Select
            value={projectId}
            onChange={setProjectId}
            options={[
              { value: 'all', label: 'All projects' },
              ...(projects ?? []).map((project) => ({
                value: project.id,
                label: project.displayName,
              })),
            ]}
          />
        </div>
      </div>

      {failure && (
        <Card className="mb-4 border-danger/30 bg-danger-soft px-4 py-3">
          <p className="text-[13px] text-danger">{failure}</p>
        </Card>
      )}

      {entries === null ? (
        <Card className="space-y-2 p-4">
          <Skeleton className="h-5 w-1/3" />
          <Skeleton className="h-5 w-2/3" />
          <Skeleton className="h-5 w-1/2" />
        </Card>
      ) : visible.length === 0 ? (
        <Card>
          <EmptyState
            icon="activity"
            title={entries.length === 0 ? 'No activity yet' : 'Nothing matches'}
            description={
              entries.length === 0
                ? 'Starting, stopping and editing projects will show up here as it happens.'
                : 'No entry matches the current search and filters.'
            }
            actions={
              entries.length > 0 ? (
                <Button
                  onClick={() => {
                    setQuery('');
                    setResult('all');
                    setProjectId('all');
                  }}
                >
                  Clear filters
                </Button>
              ) : undefined
            }
          />
        </Card>
      ) : (
        <div className="space-y-4">
          {groups.map(([day, items]) => (
            <Card key={day} className="overflow-hidden">
              <p className="border-b border-edge px-4 py-2 text-[12px] font-medium text-muted">
                {day}
              </p>
              <ul>
                {items.map((entry) => {
                  const tone = actionTone(entry.result);
                  const project =
                    entry.targetType === 'project'
                      ? projects?.find((item) => item.id === entry.targetId)
                      : undefined;

                  return (
                    <li
                      key={entry.id}
                      className="flex items-center gap-3 border-b border-edge/60 px-4 py-2 last:border-b-0"
                    >
                      <span
                        aria-hidden
                        className={`shrink-0 ${
                          tone === 'danger'
                            ? 'text-danger'
                            : tone === 'warn'
                              ? 'text-warn'
                              : 'text-ok'
                        }`}
                      >
                        <Icon
                          name={tone === 'ok' ? 'check-circle' : tone === 'warn' ? 'info' : 'alert'}
                          size={15}
                        />
                      </span>

                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] text-ink">
                          {describeAction(entry.action, entry.targetLabel)}
                        </span>
                        {entry.errorCode && (
                          <span className="block truncate text-[12px] text-danger">
                            {entry.errorCode}
                          </span>
                        )}
                      </span>

                      {project && (
                        <button
                          type="button"
                          onClick={() => onOpenProject(project.id)}
                          className="shrink-0 text-[12px] text-accent hover:underline"
                        >
                          Open
                        </button>
                      )}

                      {tone !== 'ok' && <Badge tone={tone}>{entry.result.toLowerCase()}</Badge>}

                      <span
                        title={formatTimestamp(entry.occurredAt)}
                        className="w-[76px] shrink-0 text-right text-[12px] text-faint"
                      >
                        {formatRelative(entry.occurredAt)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </Card>
          ))}

          {entries.length >= PAGE_SIZE && (
            <p className="text-center text-[12px] text-faint">
              Showing the most recent {PAGE_SIZE} entries.
            </p>
          )}
        </div>
      )}
    </PageShell>
  );
}
