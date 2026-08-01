/**
 * What a project's state means, and how to say it.
 *
 * The core stores statuses as `RUNNING`, `BUILD_FAILED` and so on. Deciding
 * once, here, which of those is worth alarming about is what keeps the
 * dashboard, the project list and the detail page from disagreeing about
 * whether a project is fine.
 */
import type { ProjectSummary } from '../api';
import type { Tone } from '../ui/primitives';

export interface StatusLook {
  label: string;
  tone: Tone;
  /** True while the core is mid-transition, so controls disable. */
  transitioning: boolean;
}

/**
 * Stopped is neutral, not red.
 *
 * Stopping a project is a normal thing to do on purpose; colouring it as a
 * fault teaches people to ignore red.
 */
export function statusLook(status: string): StatusLook {
  switch (status.toUpperCase()) {
    case 'RUNNING':
      return { label: 'Running', tone: 'ok', transitioning: false };
    case 'STARTING':
      return { label: 'Starting', tone: 'accent', transitioning: true };
    case 'STOPPING':
      return { label: 'Stopping', tone: 'accent', transitioning: true };
    case 'RESTARTING':
      return { label: 'Restarting', tone: 'accent', transitioning: true };
    case 'BUILDING':
      return { label: 'Building', tone: 'accent', transitioning: true };
    case 'DELETING':
      return { label: 'Deleting', tone: 'warn', transitioning: true };
    case 'STOPPED':
      return { label: 'Stopped', tone: 'neutral', transitioning: false };
    case 'CREATED':
      return { label: 'Created', tone: 'neutral', transitioning: false };
    case 'FAILED':
      return { label: 'Failed', tone: 'danger', transitioning: false };
    case 'BUILD_FAILED':
      return { label: 'Build failed', tone: 'danger', transitioning: false };
    case 'CRASHED':
      return { label: 'Crashed', tone: 'danger', transitioning: false };
    default:
      return { label: sentenceCase(status), tone: 'neutral', transitioning: false };
  }
}

/** The health column, which is separate from the status. */
export function healthLook(health: string): StatusLook {
  switch (health.toUpperCase()) {
    case 'HEALTHY':
      return { label: 'Healthy', tone: 'ok', transitioning: false };
    case 'UNHEALTHY':
      return { label: 'Unhealthy', tone: 'danger', transitioning: false };
    case 'STARTING':
      return { label: 'Checking', tone: 'accent', transitioning: true };
    case 'NONE':
    case 'UNKNOWN':
      return { label: 'No health check', tone: 'neutral', transitioning: false };
    default:
      return { label: sentenceCase(health), tone: 'neutral', transitioning: false };
  }
}

export function isRunning(status: string): boolean {
  return status.toUpperCase() === 'RUNNING';
}

export function isFailed(status: string): boolean {
  return ['FAILED', 'BUILD_FAILED', 'CRASHED'].includes(status.toUpperCase());
}

/**
 * A project the user should look at.
 *
 * Two cases: it failed, or it is supposed to be running and is not. The second
 * is the one that matters — a project whose desired state is RUNNING but whose
 * status is STOPPED has stopped without being asked to.
 */
export function needsAttention(project: ProjectSummary): boolean {
  if (isFailed(project.status)) return true;
  const wantsToRun = project.desiredState.toUpperCase() === 'RUNNING';
  const look = statusLook(project.status);
  return wantsToRun && !isRunning(project.status) && !look.transitioning;
}

/** Why it needs attention, in words. */
export function attentionReason(project: ProjectSummary): string {
  if (isFailed(project.status)) return statusLook(project.status).label;
  return 'Should be running';
}

export interface StatusCounts {
  total: number;
  running: number;
  stopped: number;
  failed: number;
  attention: number;
}

export function countByStatus(projects: ProjectSummary[]): StatusCounts {
  return {
    total: projects.length,
    running: projects.filter((project) => isRunning(project.status)).length,
    failed: projects.filter((project) => isFailed(project.status)).length,
    // Anything not running and not failed: stopped, created, or mid-transition.
    stopped: projects.filter((project) => !isRunning(project.status) && !isFailed(project.status))
      .length,
    attention: projects.filter(needsAttention).length,
  };
}

/**
 * The audit log's `project.start` becomes "Started project".
 *
 * The log is written for machines to filter and people to read, and the raw
 * dotted form is the wrong half of that in a feed.
 */
export function describeAction(action: string, targetLabel: string | null): string {
  const known: Record<string, string> = {
    'project.create': 'Created project',
    'project.start': 'Started project',
    'project.stop': 'Stopped project',
    'project.restart': 'Restarted project',
    'project.kill': 'Killed project',
    'project.delete': 'Deleted project',
    'project.update': 'Updated project',
    'file.write': 'Saved file',
    'file.create': 'Created file',
    'file.delete': 'Deleted file',
    'file.rename': 'Renamed file',
    'file.upload': 'Uploaded file',
    'file.import': 'Imported files',
    'app.start': 'Application started',
    'app.shutdown': 'Application stopped',
  };

  const phrase = known[action] ?? sentenceCase(action.replace(/[._]/g, ' '));
  return targetLabel ? `${phrase} ${targetLabel}` : phrase;
}

export function actionTone(result: string): Tone {
  switch (result.toUpperCase()) {
    case 'SUCCESS':
    case 'OK':
      return 'ok';
    case 'FAILURE':
    case 'FAILED':
    case 'ERROR':
      return 'danger';
    case 'DENIED':
      return 'warn';
    default:
      return 'neutral';
  }
}

function sentenceCase(value: string): string {
  const lower = value.toLowerCase().replace(/_/g, ' ');
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}
