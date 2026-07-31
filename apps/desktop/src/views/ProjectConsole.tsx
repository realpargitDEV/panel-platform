import { useState } from 'react';

import {
  errorMessage,
  killProject,
  restartProject,
  startProject,
  stopProject,
  type ProjectSummary,
} from '../api';

type ConsoleAction = 'start' | 'restart' | 'stop' | 'kill';

/**
 * The console screen for one project.
 *
 * Modelled on the panel's server view: a wide log pane with a toolbar and a
 * command box, an information panel down the right, and resource cards along
 * the bottom.
 *
 * Nothing here invents numbers. Log streaming and metrics collection are not
 * built, so those areas say so rather than showing a convincing zero — a
 * dashboard reporting 0% CPU for a project nobody is measuring is worse than
 * one admitting it does not know.
 */
export default function ProjectConsole({
  project,
  dockerAvailable,
  onRefresh,
  onBack,
  onOpenFiles,
}: {
  project: ProjectSummary;
  dockerAvailable: boolean;
  onRefresh: () => Promise<void>;
  onBack: () => void;
  onOpenFiles: () => void;
}) {
  const [busyAction, setBusyAction] = useState<ConsoleAction | null>(null);
  const [actionMessage, setActionMessage] = useState<{
    tone: 'success' | 'error';
    text: string;
  } | null>(null);
  const isRunning = project.status === 'RUNNING';
  const isTransitioning = ['STARTING', 'STOPPING', 'RESTARTING'].includes(project.status);
  const isBusy = busyAction !== null;
  const controlsDisabled = isBusy || isTransitioning || !dockerAvailable;

  async function act(
    actionName: ConsoleAction,
    successMessage: string,
    action: (projectId: string) => Promise<unknown>,
  ) {
    setBusyAction(actionName);
    setActionMessage(null);
    try {
      await action(project.id);
      await onRefresh();
      setActionMessage({ tone: 'success', text: successMessage });
    } catch (error) {
      setActionMessage({ tone: 'error', text: errorMessage(error) });
    } finally {
      setBusyAction(null);
    }
  }

  return (
    <div className="px-8 py-7">
      <button
        type="button"
        onClick={onBack}
        className="text-sm text-neutral-400 hover:text-neutral-200"
      >
        ← Projects
      </button>

      <div className="mt-4 flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="text-xs font-semibold tracking-wider text-accent uppercase">Console</p>
          <h1 className="mt-1 text-3xl font-bold tracking-tight">{project.displayName}</h1>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ConsoleActionButton
            label={busyAction === 'start' ? 'Starting...' : 'Start'}
            icon="▶"
            tone="start"
            disabled={controlsDisabled || isRunning}
            title={controlTitle(dockerAvailable, isTransitioning, 'Start this project')}
            onClick={() => void act('start', 'Project started.', startProject)}
          />
          <ConsoleActionButton
            label={busyAction === 'restart' ? 'Restarting...' : 'Restart'}
            icon="↻"
            tone="restart"
            disabled={controlsDisabled || !isRunning}
            title={controlTitle(dockerAvailable, isTransitioning, 'Restart this project')}
            onClick={() => void act('restart', 'Project restarted.', restartProject)}
          />
          <ConsoleActionButton
            label={busyAction === 'stop' ? 'Stopping...' : 'Stop'}
            icon="■"
            tone="stop"
            disabled={controlsDisabled || !isRunning}
            title={controlTitle(dockerAvailable, isTransitioning, 'Stop this project gracefully')}
            onClick={() => void act('stop', 'Project stopped.', stopProject)}
          />
          <ConsoleActionButton
            label={busyAction === 'kill' ? 'Killing...' : 'Kill'}
            icon="⏻"
            tone="kill"
            disabled={controlsDisabled || !isRunning}
            title={controlTitle(dockerAvailable, isTransitioning, 'Kill this project immediately')}
            onClick={() => void act('kill', 'Project killed.', killProject)}
          />
          <button
            type="button"
            onClick={onOpenFiles}
            className="rounded-lg border border-edge bg-raised px-3.5 py-2 text-sm hover:border-white/25"
          >
            Edit files
          </button>
        </div>
      </div>

      {actionMessage && (
        <p
          className={`mt-4 rounded-lg border px-4 py-3 text-sm ${
            actionMessage.tone === 'success'
              ? 'border-emerald-900 bg-emerald-950/50 text-emerald-200'
              : 'border-red-900 bg-red-950/60 text-red-200'
          }`}
        >
          {actionMessage.text}
        </p>
      )}

      <div className="mt-6 grid gap-4 lg:grid-cols-[1fr_320px]">
        <section className="overflow-hidden rounded-xl border border-edge bg-surface">
          <div className="flex flex-wrap gap-2 border-b border-edge px-4 py-3">
            {['Copy all', 'Download', 'Hide timestamps', 'Clear'].map((label) => (
              <button
                key={label}
                type="button"
                disabled
                title="Log streaming is not built yet"
                className="rounded-md border border-edge bg-raised px-3 py-1.5 text-xs text-neutral-300 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {label}
              </button>
            ))}
          </div>

          <div className="h-80 overflow-y-auto bg-black/40 p-4 font-mono text-xs leading-relaxed text-neutral-400">
            <p>[--:--:--] Log streaming is not implemented yet.</p>
            <p>[--:--:--] This project is {project.status.toLowerCase()}.</p>
          </div>

          <div className="flex gap-2 border-t border-edge p-3">
            <input
              disabled
              placeholder="Type a command…"
              className="flex-1 rounded-md border border-edge bg-black/30 px-3 py-2 font-mono text-sm outline-none select-text disabled:cursor-not-allowed disabled:opacity-50"
            />
            <button
              type="button"
              disabled
              title="Sending commands to a running project is not built yet"
              className="rounded-md bg-accent px-4 py-2 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-50"
            >
              Send
            </button>
          </div>
        </section>

        <aside>
          <p className="mb-2 px-1 text-[11px] font-semibold tracking-wider text-neutral-500 uppercase">
            Project info
          </p>
          <div className="space-y-1.5">
            <InfoRow icon="▣" label="Status" value={project.status.toLowerCase()} />
            <InfoRow icon="◈" label="Wanted" value={project.desiredState.toLowerCase()} />
            <InfoRow icon="⌘" label="Slug" value={project.slug} />
            <InfoRow icon="▤" label="Type" value={project.projectType.toLowerCase()} />
            <InfoRow icon="◷" label="Uptime" value="not measured" muted />
            <InfoRow icon="◐" label="Memory" value="not measured" muted />
          </div>
        </aside>
      </div>

      <div className="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <ResourceCard icon="◉" tint="bg-emerald-500" title="CPU" caption="Processing power" />
        <ResourceCard icon="◐" tint="bg-accent" title="Memory" caption="Current usage" />
        <ResourceCard icon="⇅" tint="bg-purple-500" title="Network" caption="Data transfer" />
        <ResourceCard icon="▤" tint="bg-orange-500" title="Disk" caption="Storage space" />
      </div>
    </div>
  );
}

function controlTitle(dockerAvailable: boolean, isTransitioning: boolean, action: string): string {
  if (!dockerAvailable) return 'Docker is not available';
  if (isTransitioning) return 'Project is already changing state';
  return action;
}

function ConsoleActionButton({
  label,
  icon,
  tone,
  disabled,
  title,
  onClick,
}: {
  label: string;
  icon: string;
  tone: 'start' | 'restart' | 'stop' | 'kill';
  disabled: boolean;
  title: string;
  onClick: () => void;
}) {
  const tones = {
    start: 'text-emerald-300 shadow-emerald-950/40',
    restart: 'text-sky-300 shadow-sky-950/40',
    stop: 'text-neutral-300 shadow-neutral-950/40',
    kill: 'text-red-300 shadow-red-950/40',
  };

  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={`inline-flex items-center gap-2 rounded-md border border-white/10 bg-[#242935] px-3 py-2 text-sm font-medium text-neutral-100 shadow-lg transition hover:border-white/25 hover:bg-[#2d3341] disabled:cursor-not-allowed disabled:opacity-40 ${tones[tone]}`}
    >
      <span aria-hidden className="text-base leading-none">
        {icon}
      </span>
      <span>{label}</span>
    </button>
  );
}

function InfoRow({
  icon,
  label,
  value,
  muted,
}: {
  icon: string;
  label: string;
  value: string;
  muted?: boolean;
}) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-edge bg-raised px-3 py-2.5">
      <span aria-hidden className="w-4 text-center text-xs text-neutral-500">
        {icon}
      </span>
      <span className="flex-1 text-sm text-neutral-400">{label}</span>
      <span className={`text-sm font-medium ${muted ? 'text-neutral-600' : 'text-neutral-100'}`}>
        {value}
      </span>
    </div>
  );
}

function ResourceCard({
  icon,
  tint,
  title,
  caption,
}: {
  icon: string;
  tint: string;
  title: string;
  caption: string;
}) {
  return (
    <div className="card-hover flex items-center gap-3 rounded-xl border border-edge bg-surface px-4 py-4">
      <span
        aria-hidden
        className={`grid h-10 w-10 shrink-0 place-items-center rounded-xl ${tint} text-sm shadow-lg`}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block leading-tight font-semibold">{title}</span>
        <span className="block text-[10px] font-semibold tracking-wider text-neutral-500 uppercase">
          {caption}
        </span>
      </span>
      {/* Deliberately not a number. Metrics collection is Phase 6. */}
      <span className="text-lg font-semibold text-neutral-600">—</span>
    </div>
  );
}
