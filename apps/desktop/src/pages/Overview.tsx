/**
 * The dashboard.
 *
 * Answers three questions in order: is the machine healthy, is anything broken,
 * and what was I doing. Everything on it is measured or recorded — the resource
 * meters come from the host, the activity feed from the audit log the core
 * already writes, and the attention list from the projects' own desired state.
 */
import { useEffect, useState } from 'react';

import {
  recentActivity,
  systemMetrics,
  type ActivityEntry,
  type ProjectSummary,
  type SystemMetrics,
  type SystemStatus,
} from '../api';
import { formatBytes, formatDuration, formatRelative, percentOf } from '../lib/format';
import {
  actionTone,
  attentionReason,
  countByStatus,
  describeAction,
  needsAttention,
  statusLook,
} from '../lib/projects';
import ProjectMark from '../ui/ProjectMark';
import Icon from '../ui/Icon';
import {
  Badge,
  Button,
  Card,
  CardHeader,
  DataRow,
  EmptyState,
  Meter,
  PageShell,
  Skeleton,
  Stat,
} from '../ui/primitives';

/** How often the resource meters resample. */
const METRICS_MS = 4000;

export default function Overview({
  status,
  projects,
  recentIds,
  dockerDismissed,
  onDismissDocker,
  onOpenProject,
  onNewProject,
  onGoProjects,
  onGoActivity,
  onGoSettings,
  onRetryDocker,
}: {
  status: SystemStatus | null;
  projects: ProjectSummary[] | null;
  recentIds: string[];
  dockerDismissed: boolean;
  onDismissDocker: () => void;
  onOpenProject: (id: string) => void;
  onNewProject: () => void;
  onGoProjects: () => void;
  onGoActivity: () => void;
  onGoSettings: () => void;
  onRetryDocker: () => void;
}) {
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);
  const [metricsFailed, setMetricsFailed] = useState(false);
  const [activity, setActivity] = useState<ActivityEntry[] | null>(null);

  useEffect(() => {
    let live = true;
    const sample = () => {
      systemMetrics()
        .then((next) => {
          if (!live) return;
          setMetrics(next);
          setMetricsFailed(false);
        })
        .catch(() => live && setMetricsFailed(true));
    };
    sample();
    const timer = setInterval(sample, METRICS_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    let live = true;
    recentActivity(8)
      .then((entries) => live && setActivity(entries))
      .catch(() => live && setActivity([]));
    return () => {
      live = false;
    };
  }, [projects]);

  const counts = countByStatus(projects ?? []);
  const attention = (projects ?? []).filter(needsAttention);
  const recent = recentIds
    .map((id) => projects?.find((project) => project.id === id))
    .filter((project): project is ProjectSummary => project !== undefined);

  const dockerDown = status !== null && !status.dockerAvailable;

  return (
    <PageShell
      title="Overview"
      description="How this machine and its projects are doing."
      actions={
        <Button variant="primary" icon="plus" onClick={onNewProject}>
          New project
        </Button>
      }
    >
      {/* Compact and dismissible. Docker being down is a degraded state, not a
          reason to bury the rest of the page. */}
      {dockerDown && !dockerDismissed && (
        <div className="mb-6 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-[12px] border border-warn/30 bg-warn-soft px-4 py-3">
          <span className="h-2 w-2 shrink-0 rounded-full bg-warn" aria-hidden />
          <div className="min-w-[220px] flex-1">
            <p className="text-[13px] font-medium text-ink">Docker is not running</p>
            <p className="mt-0.5 text-[12px] text-muted">
              Projects cannot start until Docker is available. Creating projects, editing files and
              settings still work.
            </p>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button size="sm" icon="refresh" onClick={onRetryDocker}>
              Retry
            </Button>
            <Button size="sm" onClick={onGoSettings}>
              Learn more
            </Button>
            <Button size="sm" variant="ghost" onClick={onDismissDocker}>
              Dismiss
            </Button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat
          label="Projects"
          value={projects === null ? '—' : counts.total}
          onClick={onGoProjects}
        />
        <Stat
          label="Running"
          value={projects === null ? '—' : counts.running}
          tone={counts.running > 0 ? 'ok' : undefined}
          onClick={onGoProjects}
        />
        <Stat
          label="Stopped"
          value={projects === null ? '—' : counts.stopped}
          onClick={onGoProjects}
        />
        <Stat
          label="Failed"
          value={projects === null ? '—' : counts.failed}
          tone={counts.failed > 0 ? 'danger' : undefined}
          onClick={onGoProjects}
        />
      </div>

      <div className="mt-6 grid gap-4 lg:grid-cols-[1.4fr_1fr]">
        <div className="space-y-4">
          <Card>
            <CardHeader
              title="This machine"
              subtitle={
                metrics
                  ? `${metrics.cpuCount} cores · sampled every ${METRICS_MS / 1000}s`
                  : 'Sampling…'
              }
            />
            <div className="grid gap-5 px-4 py-4 sm:grid-cols-3">
              {metricsFailed ? (
                <p className="text-[13px] text-muted sm:col-span-3">
                  The machine&apos;s resource usage could not be read.
                </p>
              ) : metrics === null ? (
                <>
                  <Skeleton className="h-10" />
                  <Skeleton className="h-10" />
                  <Skeleton className="h-10" />
                </>
              ) : (
                <>
                  <Meter label="CPU" value={metrics.cpuPercent} />
                  <Meter
                    label="Memory"
                    value={percentOf(metrics.memoryUsedBytes, metrics.memoryTotalBytes)}
                    caption={`${formatBytes(metrics.memoryUsedBytes)} of ${formatBytes(metrics.memoryTotalBytes)}`}
                  />
                  <Meter
                    label="Disk"
                    value={percentOf(metrics.diskUsedBytes, metrics.diskTotalBytes)}
                    caption={`${formatBytes(metrics.diskUsedBytes)} of ${formatBytes(metrics.diskTotalBytes)}`}
                  />
                </>
              )}
            </div>
            <div className="border-t border-edge px-4 py-1">
              <DataRow
                label="Application uptime"
                value={status ? formatDuration(status.uptimeSeconds) : '—'}
              />
              <DataRow
                label="Docker"
                value={
                  status ? (
                    <Badge tone={status.dockerAvailable ? 'ok' : 'warn'} dot>
                      {status.dockerAvailable ? 'Connected' : 'Unavailable'}
                    </Badge>
                  ) : (
                    '—'
                  )
                }
              />
              {metrics && metrics.diskMount && (
                <DataRow label="Projects volume" value={metrics.diskMount} mono />
              )}
            </div>
          </Card>

          <Card>
            <CardHeader
              title="Recent activity"
              actions={
                <Button size="sm" variant="ghost" onClick={onGoActivity}>
                  View all
                </Button>
              }
            />
            {activity === null ? (
              <div className="space-y-2 px-4 py-4">
                <Skeleton className="h-4 w-2/3" />
                <Skeleton className="h-4 w-1/2" />
                <Skeleton className="h-4 w-3/5" />
              </div>
            ) : activity.length === 0 ? (
              <p className="px-4 py-6 text-center text-[13px] text-muted">
                Nothing has happened yet.
              </p>
            ) : (
              <ul className="px-4 py-1">
                {activity.map((entry) => (
                  <li
                    key={entry.id}
                    className="flex items-center gap-3 border-b border-edge/60 py-2 last:border-b-0"
                  >
                    <span
                      aria-hidden
                      className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                        actionTone(entry.result) === 'danger'
                          ? 'bg-danger'
                          : actionTone(entry.result) === 'warn'
                            ? 'bg-warn'
                            : 'bg-ok'
                      }`}
                    />
                    <span className="min-w-0 flex-1 truncate text-[13px] text-ink">
                      {describeAction(entry.action, entry.targetLabel)}
                    </span>
                    <span className="shrink-0 text-[12px] text-faint">
                      {formatRelative(entry.occurredAt)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader title="Needs attention" />
            {projects === null ? (
              <div className="space-y-2 px-4 py-4">
                <Skeleton className="h-8" />
                <Skeleton className="h-8" />
              </div>
            ) : attention.length === 0 ? (
              <div className="flex items-center gap-2.5 px-4 py-5 text-[13px] text-muted">
                <span className="text-ok">
                  <Icon name="check-circle" size={16} />
                </span>
                Everything is behaving.
              </div>
            ) : (
              <ul className="p-2">
                {attention.map((project) => (
                  <li key={project.id}>
                    <button
                      type="button"
                      onClick={() => onOpenProject(project.id)}
                      className="flex w-full items-center gap-2.5 rounded-[8px] px-2 py-2 text-left hover:bg-raised"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] text-ink">
                          {project.displayName}
                        </span>
                        <span className="block truncate text-[12px] text-muted">
                          {attentionReason(project)}
                        </span>
                      </span>
                      <Badge tone={statusLook(project.status).tone} dot>
                        {statusLook(project.status).label}
                      </Badge>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Card>

          <Card>
            <CardHeader title="Recently opened" />
            {recent.length === 0 ? (
              <p className="px-4 py-5 text-[13px] text-muted">
                Projects you open will be listed here.
              </p>
            ) : (
              <ul className="p-2">
                {recent.map((project) => (
                  <li key={project.id}>
                    <button
                      type="button"
                      onClick={() => onOpenProject(project.id)}
                      className="flex w-full items-center gap-2.5 rounded-[8px] px-2 py-2 text-left hover:bg-raised"
                    >
                      <ProjectMark projectId={project.id} runtime={project.projectType} size={24} />
                      <span className="min-w-0 flex-1 truncate text-[13px] text-ink">
                        {project.displayName}
                      </span>
                      <span
                        aria-hidden
                        className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                          statusLook(project.status).tone === 'ok' ? 'bg-ok' : 'bg-faint'
                        }`}
                      />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>
      </div>

      {projects !== null && projects.length === 0 && (
        <Card className="mt-4">
          <EmptyState
            icon="projects"
            title="No projects yet"
            description="A project is a folder on this machine that Panel Platform can build, run and keep running."
            actions={
              <Button variant="primary" icon="plus" onClick={onNewProject}>
                Create your first project
              </Button>
            }
          />
        </Card>
      )}
    </PageShell>
  );
}
