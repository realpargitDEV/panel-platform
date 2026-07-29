import { useEffect, useState } from 'react';
import {
  errorMessage,
  restartProject,
  startProject,
  stopProject,
  type ProjectSummary,
} from '../api';
import PageHeader from '../components/PageHeader';

const RUNTIMES = [
  { id: 'NODEJS', label: 'Node.js', hint: 'Discord bots, APIs, workers' },
  { id: 'PYTHON', label: 'Python', hint: 'Bots, scripts, APIs' },
  { id: 'STATIC', label: 'Static site', hint: 'HTML, CSS and JavaScript' },
];

export default function Projects({
  projects,
  dockerAvailable,
  onCreate,
  onRefresh,
  createRequested,
  onOpen,
}: {
  projects: ProjectSummary[] | null;
  dockerAvailable: boolean;
  onCreate: (name: string, description: string, runtime: string) => Promise<void>;
  onRefresh: () => Promise<void>;
  createRequested: number;
  onOpen: (id: string) => void;
}) {
  const [creating, setCreating] = useState(false);

  // The sidebar can ask for the dialog; a counter rather than a boolean so a
  // second press reopens it after a cancel.
  useEffect(() => {
    if (createRequested > 0) setCreating(true);
  }, [createRequested]);
  // Which project is mid-operation. A first start builds an image, which takes
  // minutes, so the button must not look idle while that happens.
  const [busy, setBusy] = useState<string | null>(null);
  const [actionFailure, setActionFailure] = useState<string | null>(null);

  async function act(id: string, action: (id: string) => Promise<unknown>) {
    setBusy(id);
    setActionFailure(null);
    try {
      await action(id);
      await onRefresh();
    } catch (error) {
      setActionFailure(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="px-8 py-7">
      <div className="flex items-start justify-between">
        <PageHeader
          breadcrumb="Projects"
          label="Your projects"
          title="Projects"
          subtitle={
            projects === null
              ? 'Loading…'
              : `${projects.length} project${projects.length === 1 ? '' : 's'} on this machine`
          }
        />
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium hover:brightness-110"
        >
          New project
        </button>
      </div>

      {projects !== null && projects.length === 0 && (
        <section className="mt-8 rounded-xl border border-dashed border-edge p-12 text-center">
          <h2 className="font-medium">Nothing here yet</h2>
          <p className="mx-auto mt-2 max-w-md text-sm leading-relaxed text-neutral-400">
            Create a project to get started. You can create one whether or not Docker is running —
            it just will not start until Docker is available.
          </p>
        </section>
      )}

      {projects !== null && projects.length > 0 && (
        <ul className="mt-6 space-y-2">
          {projects.map((project) => (
            <li
              key={project.id}
              className="card-hover flex items-center gap-4 rounded-xl border border-edge bg-surface px-4 py-4"
            >
              <span
                className="grid h-10 w-10 shrink-0 place-items-center rounded-xl text-sm font-semibold shadow-lg"
                style={{ background: project.color ?? '#2f6bff' }}
                aria-hidden
              >
                {project.displayName.slice(0, 1).toUpperCase()}
              </span>
              <button
                type="button"
                onClick={() => onOpen(project.id)}
                className="min-w-0 flex-1 text-left"
              >
                <p className="truncate font-medium hover:text-accent">{project.displayName}</p>
                <p className="truncate text-sm text-neutral-500">
                  {project.description || project.slug}
                </p>
              </button>
              <StatusPill status={project.status} />
              {project.status === 'RUNNING' ? (
                <>
                  <button
                    type="button"
                    onClick={() => void act(project.id, restartProject)}
                    disabled={busy !== null}
                    className="rounded-md border border-edge px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    Restart
                  </button>
                  <button
                    type="button"
                    onClick={() => void act(project.id, stopProject)}
                    disabled={busy !== null}
                    className="rounded-md border border-edge px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40"
                  >
                    {busy === project.id ? 'Working…' : 'Stop'}
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  onClick={() => void act(project.id, startProject)}
                  disabled={!dockerAvailable || busy !== null}
                  title={dockerAvailable ? 'Start this project' : 'Docker is not available'}
                  className="rounded-md border border-edge px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {busy === project.id ? 'Starting…' : 'Start'}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}

      {actionFailure && (
        <p className="mt-4 rounded-lg border border-red-900 bg-red-950/60 px-4 py-3 text-sm text-red-200">
          {actionFailure}
        </p>
      )}

      {creating && <CreateDialog onClose={() => setCreating(false)} onCreate={onCreate} />}
    </div>
  );
}

function CreateDialog({
  onClose,
  onCreate,
}: {
  onClose: () => void;
  onCreate: (name: string, description: string, runtime: string) => Promise<void>;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [runtime, setRuntime] = useState('NODEJS');
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setFailure(null);
    try {
      await onCreate(name, description, runtime);
      onClose();
    } catch (error) {
      // Shown in the dialog rather than closing it, so nothing typed is lost.
      setFailure(errorMessage(error));
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 grid place-items-center bg-black/60 p-6">
      <form
        onSubmit={submit}
        className="w-full max-w-md rounded-xl border border-edge bg-raised p-6 shadow-2xl"
      >
        <h2 className="text-lg font-semibold">New project</h2>

        <label className="mt-5 block text-sm">
          <span className="text-neutral-300">Name</span>
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoFocus
            maxLength={60}
            placeholder="My Discord bot"
            className="mt-1.5 w-full rounded-md border border-edge bg-black/30 px-3 py-2 outline-none select-text focus:border-accent"
          />
        </label>

        <label className="mt-4 block text-sm">
          <span className="text-neutral-300">Description</span>
          <input
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            maxLength={200}
            placeholder="Optional"
            className="mt-1.5 w-full rounded-md border border-edge bg-black/30 px-3 py-2 outline-none select-text focus:border-accent"
          />
        </label>

        <fieldset className="mt-4">
          <legend className="text-sm text-neutral-300">Runtime</legend>
          <div className="mt-2 space-y-2">
            {RUNTIMES.map((option) => (
              <label
                key={option.id}
                className={`flex cursor-pointer items-center gap-3 rounded-md border px-3 py-2.5 text-sm ${
                  runtime === option.id
                    ? 'border-accent bg-accent/10'
                    : 'border-edge hover:border-white/25'
                }`}
              >
                <input
                  type="radio"
                  name="runtime"
                  value={option.id}
                  checked={runtime === option.id}
                  onChange={() => setRuntime(option.id)}
                  className="accent-[#2f6bff]"
                />
                <span className="flex-1">
                  <span className="font-medium">{option.label}</span>
                  <span className="ml-2 text-neutral-500">{option.hint}</span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        {failure && (
          <p className="mt-4 rounded-md border border-red-900 bg-red-950/60 px-3 py-2 text-sm text-red-200">
            {failure}
          </p>
        )}

        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-4 py-2 text-sm text-neutral-300 hover:bg-white/5"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy || name.trim().length === 0}
            className="rounded-md bg-accent px-4 py-2 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-50"
          >
            {busy ? 'Creating…' : 'Create'}
          </button>
        </div>
      </form>
    </div>
  );
}

function StatusPill({ status }: { status: string }) {
  const tone =
    status === 'RUNNING'
      ? 'bg-emerald-950 text-emerald-300'
      : status === 'FAILED'
        ? 'bg-red-950 text-red-300'
        : 'bg-white/10 text-neutral-300';

  return (
    <span className={`rounded-full px-2.5 py-1 text-xs font-medium ${tone}`}>
      {status.toLowerCase().replace(/_/g, ' ')}
    </span>
  );
}
