/**
 * The project list.
 *
 * Two shapes of the same data: a grid of cards for browsing, and a dense table
 * for scanning many. Which one is remembered, along with the filters, because
 * someone who works in the list view is not choosing it again every morning.
 *
 * The card shows what the core actually knows about a project. Live CPU and
 * memory per project are not among them — that needs Docker's stats stream,
 * which the manager does not read — so the card does not pretend to have them.
 */
import { useEffect, useMemo, useState } from 'react';

import {
  killProject,
  restartProject,
  startProject,
  stopProject,
  type ProjectSummary,
} from '../api';
import {
  applyListOptions,
  defaultListOptions,
  isFiltered,
  runtimeLabel,
  runtimesIn,
  type ListOptions,
  type SortKey,
  type StatusFilter,
  type ViewMode,
} from '../lib/projectList';
import { isRunning, statusLook } from '../lib/projects';
import Icon from '../ui/Icon';
import { Menu, useMenu } from '../ui/overlays';
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
import { toast } from '../ui/toast';

const VIEW_KEY = 'panel.projectsView.v1';

export default function Projects({
  projects,
  dockerAvailable,
  onRefresh,
  onOpen,
  onNewProject,
}: {
  projects: ProjectSummary[] | null;
  dockerAvailable: boolean;
  onRefresh: () => Promise<void>;
  onOpen: (id: string) => void;
  onNewProject: () => void;
}) {
  const [options, setOptions] = useState<ListOptions>(defaultListOptions);
  const [mode, setMode] = useState<ViewMode>(() =>
    typeof window === 'undefined' || window.localStorage.getItem(VIEW_KEY) === 'list'
      ? 'list'
      : 'grid',
  );
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    try {
      window.localStorage.setItem(VIEW_KEY, mode);
    } catch {
      // A storage that refuses is not worth interrupting anyone over.
    }
  }, [mode]);

  const visible = useMemo(() => applyListOptions(projects ?? [], options), [projects, options]);
  const runtimes = useMemo(() => runtimesIn(projects ?? []), [projects]);

  async function act(
    project: ProjectSummary,
    verb: string,
    action: (id: string) => Promise<unknown>,
  ) {
    setBusy(project.id);
    try {
      await action(project.id);
      await onRefresh();
      toast.success(`${project.displayName} ${verb}`);
    } catch (error) {
      toast.error(
        `Could not ${verb.replace(/ed$/, '')} ${project.displayName}`,
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setBusy(null);
    }
  }

  const patch = (next: Partial<ListOptions>) => setOptions((current) => ({ ...current, ...next }));

  return (
    <PageShell
      title="Projects"
      description={
        projects === null
          ? 'Loading…'
          : `${projects.length} project${projects.length === 1 ? '' : 's'} on this machine`
      }
      actions={
        <>
          <IconButton icon="refresh" label="Refresh" onClick={() => void onRefresh()} />
          <Button variant="primary" icon="plus" onClick={onNewProject}>
            New project
          </Button>
        </>
      }
    >
      {projects !== null && projects.length > 0 && (
        <div className="mb-4 flex flex-wrap items-end gap-2">
          <div className="min-w-[200px] flex-1">
            <TextInput
              value={options.query}
              onChange={(query) => patch({ query })}
              placeholder="Search projects"
            />
          </div>

          <div className="w-[140px]">
            <Select<StatusFilter>
              value={options.status}
              onChange={(status) => patch({ status })}
              options={[
                { value: 'all', label: 'Any status' },
                { value: 'running', label: 'Running' },
                { value: 'stopped', label: 'Stopped' },
                { value: 'failed', label: 'Failed' },
              ]}
            />
          </div>

          <div className="w-[150px]">
            <Select
              value={options.runtime}
              onChange={(runtime) => patch({ runtime })}
              options={[
                { value: 'all', label: 'Any runtime' },
                ...runtimes.map((id) => ({ value: id, label: runtimeLabel(id) })),
              ]}
            />
          </div>

          <div className="w-[150px]">
            <Select<SortKey>
              value={options.sort}
              onChange={(sort) => patch({ sort })}
              options={[
                { value: 'name', label: 'Sort by name' },
                { value: 'status', label: 'Sort by status' },
                { value: 'runtime', label: 'Sort by runtime' },
              ]}
            />
          </div>

          <div className="flex gap-1 rounded-[8px] border border-edge bg-raised p-0.5">
            <IconButton
              icon="grid"
              label="Grid view"
              size="sm"
              active={mode === 'grid'}
              onClick={() => setMode('grid')}
            />
            <IconButton
              icon="list"
              label="List view"
              size="sm"
              active={mode === 'list'}
              onClick={() => setMode('list')}
            />
          </div>
        </div>
      )}

      {projects === null && (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          <Skeleton className="h-[132px]" />
          <Skeleton className="h-[132px]" />
          <Skeleton className="h-[132px]" />
        </div>
      )}

      {projects !== null && projects.length === 0 && (
        <Card>
          <EmptyState
            icon="projects"
            title="No projects yet"
            description="A project is a folder on this machine that Panel Platform builds, runs and keeps running — a bot, an API, a website, or anything else with a start command."
            actions={
              <>
                <Button variant="primary" icon="plus" onClick={onNewProject}>
                  Create project
                </Button>
                <Button icon="git" onClick={onNewProject}>
                  Clone repository
                </Button>
                <Button icon="download" onClick={onNewProject}>
                  From archive URL
                </Button>
              </>
            }
          >
            <div className="mt-6 flex flex-wrap justify-center gap-1.5">
              {['Node.js', 'Python', 'Go', 'Rust', 'Static site', 'Dockerfile'].map((example) => (
                <span
                  key={example}
                  className="rounded-full border border-edge px-2.5 py-1 text-[12px] text-muted"
                >
                  {example}
                </span>
              ))}
            </div>
          </EmptyState>
        </Card>
      )}

      {projects !== null && projects.length > 0 && visible.length === 0 && (
        <Card>
          <EmptyState
            icon="search"
            title="Nothing matches"
            description={
              isFiltered(options)
                ? 'No project matches the current search and filters.'
                : 'There is nothing to show.'
            }
            actions={<Button onClick={() => setOptions(defaultListOptions)}>Clear filters</Button>}
          />
        </Card>
      )}

      {visible.length > 0 &&
        (mode === 'grid' ? (
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {visible.map((project) => (
              <ProjectCard
                key={project.id}
                project={project}
                busy={busy === project.id}
                dockerAvailable={dockerAvailable}
                onOpen={() => onOpen(project.id)}
                onAct={act}
              />
            ))}
          </div>
        ) : (
          <Card className="overflow-hidden">
            <ul>
              {visible.map((project) => (
                <ProjectRow
                  key={project.id}
                  project={project}
                  busy={busy === project.id}
                  dockerAvailable={dockerAvailable}
                  onOpen={() => onOpen(project.id)}
                  onAct={act}
                />
              ))}
            </ul>
          </Card>
        ))}
    </PageShell>
  );
}

type ActFn = (
  project: ProjectSummary,
  verb: string,
  action: (id: string) => Promise<unknown>,
) => Promise<void>;

/** The controls a project offers, and why each is unavailable when it is. */
function useControls(project: ProjectSummary, busy: boolean, dockerAvailable: boolean) {
  const look = statusLook(project.status);
  const running = isRunning(project.status);
  const blocked = busy || look.transitioning || !dockerAvailable;

  const reason = !dockerAvailable
    ? 'Docker is not available'
    : look.transitioning
      ? `The project is ${look.label.toLowerCase()}`
      : busy
        ? 'Another action is running'
        : undefined;

  return { look, running, blocked, reason };
}

function ProjectCard({
  project,
  busy,
  dockerAvailable,
  onOpen,
  onAct,
}: {
  project: ProjectSummary;
  busy: boolean;
  dockerAvailable: boolean;
  onOpen: () => void;
  onAct: ActFn;
}) {
  const { look, running, blocked, reason } = useControls(project, busy, dockerAvailable);
  const menu = useMenu();

  return (
    <Card interactive className="flex flex-col">
      <div className="flex items-start gap-3 p-4">
        <span
          aria-hidden
          className="grid h-8 w-8 shrink-0 place-items-center rounded-[8px] text-[13px] font-semibold text-white"
          style={{ background: project.color ?? '#3b82f6' }}
        >
          {project.displayName.slice(0, 1).toUpperCase()}
        </span>

        <button type="button" onClick={onOpen} className="min-w-0 flex-1 text-left">
          <span className="block truncate text-[14px] font-medium text-ink hover:text-accent">
            {project.displayName}
          </span>
          <span className="mt-0.5 block truncate text-[12px] text-muted">
            {project.description || project.slug}
          </span>
        </button>

        <Badge tone={look.tone} dot>
          {look.label}
        </Badge>
      </div>

      <div className="flex items-center gap-2 px-4 pb-3 text-[12px] text-muted">
        <span className="inline-flex items-center gap-1.5 rounded-full border border-edge px-2 py-0.5">
          <Icon name="container" size={12} />
          {runtimeLabel(project.projectType)}
        </span>
        {project.desiredState.toUpperCase() === 'RUNNING' && !running && (
          <span className="text-warn">wants to run</span>
        )}
      </div>

      <div className="mt-auto flex items-center gap-1.5 border-t border-edge px-3 py-2.5">
        {running ? (
          <>
            <Button
              size="sm"
              icon="stop"
              disabled={blocked}
              title={reason ?? 'Stop this project'}
              onClick={() => void onAct(project, 'stopped', stopProject)}
            >
              Stop
            </Button>
            <Button
              size="sm"
              icon="restart"
              disabled={blocked}
              title={reason ?? 'Restart this project'}
              onClick={() => void onAct(project, 'restarted', restartProject)}
            >
              Restart
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            icon="play"
            disabled={blocked}
            title={reason ?? 'Start this project'}
            onClick={() => void onAct(project, 'started', startProject)}
          >
            Start
          </Button>
        )}

        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onOpen}>
          Open
        </Button>
        <IconButton icon="more" label="More actions" size="sm" onClick={menu.open} />
      </div>

      {menu.anchor && (
        <Menu
          anchor={menu.anchor}
          onClose={menu.close}
          items={projectMenuItems(project, { running, blocked, reason }, onOpen, onAct)}
        />
      )}
    </Card>
  );
}

function ProjectRow({
  project,
  busy,
  dockerAvailable,
  onOpen,
  onAct,
}: {
  project: ProjectSummary;
  busy: boolean;
  dockerAvailable: boolean;
  onOpen: () => void;
  onAct: ActFn;
}) {
  const { look, running, blocked, reason } = useControls(project, busy, dockerAvailable);
  const menu = useMenu();

  return (
    <li className="flex items-center gap-3 border-b border-edge px-3 py-2 last:border-b-0 hover:bg-raised/50">
      <span
        aria-hidden
        className="grid h-7 w-7 shrink-0 place-items-center rounded-[6px] text-[12px] font-semibold text-white"
        style={{ background: project.color ?? '#3b82f6' }}
      >
        {project.displayName.slice(0, 1).toUpperCase()}
      </span>

      <button type="button" onClick={onOpen} className="min-w-0 flex-[2] text-left">
        <span className="block truncate text-[13px] font-medium text-ink hover:text-accent">
          {project.displayName}
        </span>
        <span className="block truncate text-[12px] text-muted">
          {project.description || project.slug}
        </span>
      </button>

      <span className="hidden min-w-0 flex-1 truncate text-[12px] text-muted md:block">
        {runtimeLabel(project.projectType)}
      </span>

      <Badge tone={look.tone} dot>
        {look.label}
      </Badge>

      <div className="flex shrink-0 items-center gap-1">
        {running ? (
          <IconButton
            icon="stop"
            label={reason ?? 'Stop'}
            size="sm"
            disabled={blocked}
            onClick={() => void onAct(project, 'stopped', stopProject)}
          />
        ) : (
          <IconButton
            icon="play"
            label={reason ?? 'Start'}
            size="sm"
            disabled={blocked}
            onClick={() => void onAct(project, 'started', startProject)}
          />
        )}
        <IconButton
          icon="restart"
          label={reason ?? 'Restart'}
          size="sm"
          disabled={blocked || !running}
          onClick={() => void onAct(project, 'restarted', restartProject)}
        />
        <IconButton icon="more" label="More actions" size="sm" onClick={menu.open} />
      </div>

      {menu.anchor && (
        <Menu
          anchor={menu.anchor}
          onClose={menu.close}
          items={projectMenuItems(project, { running, blocked, reason }, onOpen, onAct)}
        />
      )}
    </li>
  );
}

function projectMenuItems(
  project: ProjectSummary,
  state: { running: boolean; blocked: boolean; reason?: string },
  onOpen: () => void,
  onAct: ActFn,
) {
  return [
    { id: 'open', label: 'Open project', icon: 'external' as const, run: onOpen },
    {
      id: 'restart',
      label: 'Restart',
      icon: 'restart' as const,
      disabled: state.blocked || !state.running,
      reason: state.reason ?? 'The project is not running',
      run: () => void onAct(project, 'restarted', restartProject),
    },
    {
      id: 'kill',
      label: 'Force kill',
      icon: 'power' as const,
      danger: true,
      disabled: state.blocked || !state.running,
      reason: state.reason ?? 'The project is not running',
      run: () => void onAct(project, 'killed', killProject),
    },
  ];
}
