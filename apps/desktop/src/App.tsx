import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  createProject,
  errorMessage,
  listProjects,
  restartProject,
  startProject,
  stopProject,
  systemStatus,
  type ProjectSummary,
  type SystemStatus,
} from './api';
import { readRecent, recordRecent } from './lib/recent';
import { isRunning } from './lib/projects';
import { toRequest, type Draft } from './lib/wizard';
import Activity from './pages/Activity';
import Discord from './pages/Discord';
import NewProjectWizard, { CreatedSummary } from './pages/NewProjectWizard';
import Overview from './pages/Overview';
import ProjectDetail from './pages/ProjectDetail';
import Projects from './pages/Projects';
import {
  applyAppearance,
  defaultAppearance,
  effectFor,
  normaliseAppearance,
} from './lib/appearance';
import ThemeEffects from './components/ThemeEffects';
import Settings, { type Preferences } from './pages/Settings';
import CommandPalette from './shell/CommandPalette';
import Sidebar, { type View } from './shell/Sidebar';
import TopBar from './shell/TopBar';
import { isDeclined, useToolchainGate } from './components/useToolchainGate';
import UpdateManager from './components/UpdateManager';
import { installBusy } from './update';
import { toast, ToastHost } from './ui/toast';
import { updateStore, useUpdate } from './useUpdate';
import type { Command } from './workspace/commands';
import ProjectWorkspace from './workspace/ProjectWorkspace';

/** How often the shell refreshes its view of the core. */
const REFRESH_MS = 5000;

const PREFERENCES_KEY = 'panel.preferences.v1';

const defaultPreferences: Preferences = {
  collapsedSidebar: false,
  confirmDestructive: true,
  appearance: defaultAppearance,
  startupView: 'last',
  notifyStateChanges: true,
  developerMode: false,
};

function loadPreferences(): Preferences {
  try {
    const raw = window.localStorage.getItem(PREFERENCES_KEY);
    if (!raw) return defaultPreferences;
    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object') return defaultPreferences;
    const stored = parsed as Partial<Preferences>;
    return {
      collapsedSidebar:
        typeof stored.collapsedSidebar === 'boolean'
          ? stored.collapsedSidebar
          : defaultPreferences.collapsedSidebar,
      confirmDestructive:
        typeof stored.confirmDestructive === 'boolean'
          ? stored.confirmDestructive
          : defaultPreferences.confirmDestructive,
      // Validated rather than trusted: an unknown theme id written to the DOM
      // would match no token block and leave the window unstyled.
      appearance: normaliseAppearance(stored.appearance),
      startupView:
        stored.startupView === 'overview' ||
        stored.startupView === 'projects' ||
        stored.startupView === 'activity' ||
        stored.startupView === 'last'
          ? stored.startupView
          : defaultPreferences.startupView,
      notifyStateChanges:
        typeof stored.notifyStateChanges === 'boolean'
          ? stored.notifyStateChanges
          : defaultPreferences.notifyStateChanges,
      developerMode:
        typeof stored.developerMode === 'boolean'
          ? stored.developerMode
          : defaultPreferences.developerMode,
    };
  } catch {
    return defaultPreferences;
  }
}

/**
 * The application shell.
 *
 * Holds the four things every screen needs — where we are, what the core says,
 * which project is open, and this window's preferences — and nothing else. Each
 * page owns its own data beyond that.
 */
export default function App() {
  const [view, setView] = useState<View>('overview');
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const [openProject, setOpenProject] = useState<string | null>(null);
  /** True once the user asks for the editor rather than the project's pages. */
  const [editing, setEditing] = useState(false);

  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<Awaited<ReturnType<typeof createProject>> | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [dockerDismissed, setDockerDismissed] = useState(false);
  const [recentIds, setRecentIds] = useState<string[]>(() =>
    readRecent(typeof window === 'undefined' ? undefined : window.localStorage),
  );
  const [preferences, setPreferences] = useState<Preferences>(loadPreferences);

  const update = useUpdate();
  const [updatesOpen, setUpdatesOpen] = useState(false);
  /** Reported once per session, so a poll failure does not toast every 5s. */
  const reportedFailure = useRef(false);

  /**
   * Show the update manager whenever an install owns the application.
   *
   * An install can be started from four places, and one of them — the editor's
   * Help menu — is on a screen that has no room to report progress. Rather than
   * each entry point remembering to open the window, the window opens itself
   * for the only condition that requires it. A *check* deliberately does not
   * trigger this: the periodic one runs every six hours and must stay silent.
   */
  const updateOwnsApp = installBusy(update) || update.phase.state === 'installed';
  useEffect(() => {
    if (updateOwnsApp) setUpdatesOpen(true);
  }, [updateOwnsApp]);

  const openUpdates = useCallback(() => setUpdatesOpen(true), []);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextProjects] = await Promise.all([systemStatus(), listProjects()]);
      setStatus(nextStatus);
      setProjects(nextProjects);
      setFailure(null);
      reportedFailure.current = false;
    } catch (error) {
      const message = errorMessage(error);
      setFailure(message);
      if (!reportedFailure.current) {
        reportedFailure.current = true;
        toast.error('Lost contact with the core', message);
      }
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  // Update checking starts here because this component is mounted for the life
  // of the window. It used to be started by the update banner, which the top
  // bar replaced — deleting that without moving this would have ended periodic
  // checking entirely.
  //
  // A failure to *check* is deliberately silent: being unable to reach GitHub
  // is not worth interrupting anyone over, and the application is designed to
  // run with no internet at all. Settings has an explicit check that reports.
  useEffect(() => {
    updateStore.start();
    // Deliberately not stopped on unmount: the timer lives as long as the
    // window, and tearing it down here would end checking the first time React
    // remounted this in development.
  }, []);

  // Announce a project that changed state on its own — a container that fell
  // over, or one Docker restarted. Only transitions are reported, never the
  // first load, or opening the window would toast once per running project.
  const lastStatuses = useRef<Map<string, string> | null>(null);
  useEffect(() => {
    if (!projects) return;

    const current = new Map(projects.map((item) => [item.id, item.status]));
    const previous = lastStatuses.current;
    lastStatuses.current = current;

    if (!previous || !preferences.notifyStateChanges) return;

    for (const [id, status] of current) {
      const before = previous.get(id);
      if (before === undefined || before === status) continue;

      const project = projects.find((item) => item.id === id);
      if (!project) continue;

      const label = `${project.displayName} is now ${status.toLowerCase()}`;
      if (status === 'FAILED') toast.error(label, 'It stopped without being asked to.');
      else if (isRunning(status)) toast.success(label);
      else toast.info(label);
    }
  }, [projects, preferences.notifyStateChanges]);

  // Written to the document rather than threaded through the tree: a theme is
  // an attribute the token blocks in `styles.css` respond to, so switching one
  // re-paints without re-rendering anything.
  useEffect(() => {
    applyAppearance(document.documentElement, preferences.appearance);
  }, [preferences.appearance]);

  // The background the current theme asks for, if any. Read here rather than
  // inside the effects layer so the layer stays a renderer with no opinion
  // about which theme is on.
  const effect = useMemo(() => effectFor(preferences.appearance), [preferences.appearance]);

  const patchPreferences = useCallback((next: Partial<Preferences>) => {
    setPreferences((current) => {
      const merged = { ...current, ...next };
      try {
        window.localStorage.setItem(PREFERENCES_KEY, JSON.stringify(merged));
      } catch {
        // A storage that refuses is not worth interrupting anyone over.
      }
      return merged;
    });
  }, []);

  const openProjectById = useCallback((id: string) => {
    setOpenProject(id);
    setEditing(false);
    setView('projects');
    setRecentIds(recordRecent(window.localStorage, id));
  }, []);

  const project = openProject === null ? null : projects?.find((item) => item.id === openProject);
  const running = projects?.filter((item) => isRunning(item.status)).length ?? 0;
  const attention =
    projects?.filter(
      (item) => item.desiredState.toUpperCase() === 'RUNNING' && !isRunning(item.status),
    ).length ?? 0;

  const { gate, guard } = useToolchainGate();

  /** Everything the palette can run. Each one is a real action of the shell. */
  const commands = useMemo<Command[]>(() => {
    const target = project ?? null;
    return [
      {
        id: 'project.new',
        title: 'New Project',
        category: 'Projects',
        run: () => setCreating(true),
      },
      {
        id: 'go.overview',
        title: 'Go to Overview',
        category: 'Go',
        run: () => {
          setOpenProject(null);
          setView('overview');
        },
      },
      {
        id: 'go.projects',
        title: 'Go to Projects',
        category: 'Go',
        run: () => {
          setOpenProject(null);
          setView('projects');
        },
      },
      {
        id: 'go.activity',
        title: 'Go to Activity',
        category: 'Go',
        run: () => {
          setOpenProject(null);
          setView('activity');
        },
      },
      {
        id: 'go.discord',
        title: 'Go to Discord',
        category: 'Go',
        run: () => {
          setOpenProject(null);
          setView('discord');
        },
      },
      {
        id: 'go.settings',
        title: 'Open Settings',
        category: 'Go',
        run: () => {
          setOpenProject(null);
          setView('settings');
        },
      },
      {
        id: 'view.sidebar',
        title: 'Toggle Sidebar',
        category: 'View',
        keybinding: 'Ctrl+B',
        run: () => patchPreferences({ collapsedSidebar: !preferences.collapsedSidebar }),
      },
      {
        id: 'app.refresh',
        title: 'Refresh',
        category: 'Application',
        run: () => void refresh(),
      },
      {
        id: 'app.update',
        title: 'Check for Updates',
        category: 'Application',
        run: () => {
          openUpdates();
          void updateStore.check();
        },
      },
      {
        id: 'project.start',
        title: 'Start This Project',
        category: 'Projects',
        enabled: target !== null && !isRunning(target.status) && (status?.dockerAvailable ?? false),
        reason: target === null ? 'No project is open' : 'Not available right now',
        run: () => {
          if (target) void runAction(target, 'started', guard(startProject));
        },
      },
      {
        id: 'project.stop',
        title: 'Stop This Project',
        category: 'Projects',
        enabled: target !== null && isRunning(target.status),
        reason: target === null ? 'No project is open' : 'The project is not running',
        run: () => {
          if (target) void runAction(target, 'stopped', stopProject);
        },
      },
      {
        id: 'project.restart',
        title: 'Restart This Project',
        category: 'Projects',
        enabled: target !== null && isRunning(target.status),
        reason: target === null ? 'No project is open' : 'The project is not running',
        run: () => {
          if (target) void runAction(target, 'restarted', guard(restartProject));
        },
      },
      {
        id: 'project.files',
        title: 'Open Files of This Project',
        category: 'Projects',
        enabled: target !== null,
        reason: 'No project is open',
        run: () => setEditing(true),
      },
    ];

    async function runAction(
      item: ProjectSummary,
      verb: string,
      action: (id: string) => Promise<unknown>,
    ) {
      try {
        await action(item.id);
        await refresh();
        toast.success(`${item.displayName} ${verb}`);
      } catch (error) {
        // Declining an install is an answer, not a failure to report.
        if (isDeclined(error)) return;
        toast.error(
          `Could not ${verb.replace(/ed$/, '')} ${item.displayName}`,
          errorMessage(error),
        );
      }
    }
  }, [
    guard,
    patchPreferences,
    preferences.collapsedSidebar,
    project,
    refresh,
    status?.dockerAvailable,
  ]);

  // The shortcuts that belong to the shell. The editor registers its own while
  // it is mounted, and takes Ctrl+B for its side bar — so these stand down
  // whenever the workspace is what is on screen.
  useEffect(() => {
    if (editing && project) return;

    function onKeyDown(event: KeyboardEvent) {
      const modifier = event.ctrlKey || event.metaKey;
      if (!modifier) return;
      const key = event.key.toLowerCase();

      if (key === 'k' || (key === 'p' && event.shiftKey)) {
        event.preventDefault();
        setPaletteOpen(true);
      } else if (key === 'b') {
        event.preventDefault();
        patchPreferences({ collapsedSidebar: !preferences.collapsedSidebar });
      } else if (key === 'n' && !event.shiftKey) {
        event.preventDefault();
        setCreating(true);
      }
    }

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [editing, patchPreferences, preferences.collapsedSidebar, project]);

  // The editor is a complete shell of its own — its own menu bar, activity bar
  // and status bar. Nesting it inside this one would put two navigation
  // systems on screen at once.
  if (editing && project) {
    return (
      <>
        {/* No effects layer here. The editor fills the window with opaque
            panels of its own, so a canvas behind it would be a frame loop
            nobody could see. */}
        <ProjectWorkspace
          key={project.id}
          project={project}
          status={status}
          dockerAvailable={status?.dockerAvailable ?? false}
          onRefreshProjects={refresh}
          onLeave={() => setEditing(false)}
          onOpenSettings={() => {
            setEditing(false);
            setOpenProject(null);
            setView('settings');
          }}
          onOpenUpdates={openUpdates}
        />
        {/* Rendered in both shells rather than above them: the editor returns
            early, and an update started from its Help menu would otherwise have
            nowhere to report itself. */}
        <UpdateManager
          open={updatesOpen}
          currentVersion={status?.appVersion ?? '—'}
          onClose={() => setUpdatesOpen(false)}
        />
        <ToastHost />
      </>
    );
  }

  return (
    <>
      {/* Before the shell, not inside it: the layer is fixed and the shell is
          lifted above it, so the canvas colour comes from `body` and the effect
          shows through the gaps between panels rather than being painted over
          by the shell's own background. */}
      <ThemeEffects effect={effect} motion={preferences.appearance.motion} />
      <div className="theme-effects-above flex h-full text-ink">
        <Sidebar
          view={view}
          collapsed={preferences.collapsedSidebar}
          projectCount={projects?.length ?? 0}
          busyCount={
            projects?.filter((item) =>
              ['STARTING', 'STOPPING', 'RESTARTING', 'BUILDING', 'DEPLOYING'].includes(item.status),
            ).length ?? 0
          }
          dockerAvailable={status?.dockerAvailable ?? false}
          dockerSummary={status?.dockerSummary ?? 'Checking Docker…'}
          onNavigate={(next) => {
            setOpenProject(null);
            setView(next);
          }}
          onNewProject={() => setCreating(true)}
          onToggleCollapsed={() =>
            patchPreferences({ collapsedSidebar: !preferences.collapsedSidebar })
          }
        />

        <div className="flex min-w-0 flex-1 flex-col">
          <TopBar
            version={status?.appVersion ?? '—'}
            runningCount={running}
            attentionCount={attention}
            dockerAvailable={status?.dockerAvailable ?? false}
            updateAvailable={update.check?.state === 'available' ? update.check.newVersion : null}
            onOpenPalette={() => setPaletteOpen(true)}
            onOpenSettings={() => {
              setOpenProject(null);
              setView('settings');
            }}
            onCheckUpdates={() => {
              openUpdates();
              void updateStore.check();
            }}
            onOpenActivity={() => {
              setOpenProject(null);
              setView('activity');
            }}
            onInstallUpdate={openUpdates}
          />

          {/* Keyed on the destination so React remounts the subtree when the
            view changes, which is what re-runs the enter animation. Keyed on
            the open project too, or moving between two projects would swap the
            content with no transition at all. */}
          <main
            key={`${view}:${openProject ?? ''}`}
            className="animate-view min-h-0 flex-1 overflow-y-auto"
          >
            {failure && (
              <div className="border-b border-danger/30 bg-danger-soft px-8 py-2.5 text-[13px] text-danger">
                {failure}
              </div>
            )}

            {created && (
              <div className="mx-auto w-full max-w-[1200px] px-8 pt-6">
                <CreatedSummary
                  created={created}
                  onDismiss={() => setCreated(null)}
                  onOpen={() => {
                    const id = created.id;
                    setCreated(null);
                    openProjectById(id);
                  }}
                />
              </div>
            )}

            {view === 'projects' && project ? (
              <ProjectDetail
                key={project.id}
                project={project}
                dockerAvailable={status?.dockerAvailable ?? false}
                developerMode={preferences.developerMode}
                onRefreshProjects={refresh}
                onBack={() => setOpenProject(null)}
                onOpenFiles={() => setEditing(true)}
              />
            ) : view === 'overview' ? (
              <Overview
                status={status}
                projects={projects}
                recentIds={recentIds}
                dockerDismissed={dockerDismissed}
                onDismissDocker={() => setDockerDismissed(true)}
                onOpenProject={openProjectById}
                onNewProject={() => setCreating(true)}
                onGoProjects={() => setView('projects')}
                onGoActivity={() => setView('activity')}
                onGoSettings={() => setView('settings')}
                onRetryDocker={() => {
                  void refresh();
                  toast.info('Rechecking Docker');
                }}
              />
            ) : view === 'projects' ? (
              <Projects
                projects={projects}
                dockerAvailable={status?.dockerAvailable ?? false}
                onRefresh={refresh}
                onOpen={openProjectById}
                onNewProject={() => setCreating(true)}
              />
            ) : view === 'activity' ? (
              <Activity projects={projects} onOpenProject={openProjectById} />
            ) : view === 'discord' ? (
              <Discord />
            ) : (
              <Settings
                status={status}
                projects={projects}
                preferences={preferences}
                onPreferences={patchPreferences}
                onOpenUpdates={() => {
                  openUpdates();
                  void updateStore.check();
                }}
                onResetLayout={() => {
                  try {
                    window.localStorage.removeItem(PREFERENCES_KEY);
                    window.localStorage.removeItem('workspace.layout.v1');
                    window.localStorage.removeItem('panel.projectsView.v1');
                  } catch {
                    // Nothing to do: the defaults apply on the next start anyway.
                  }
                  setPreferences(defaultPreferences);
                }}
              />
            )}
          </main>
        </div>

        {creating && (
          <NewProjectWizard
            onClose={() => setCreating(false)}
            onCreate={async (draft: Draft) => {
              const result = await createProject(toRequest(draft));
              await refresh();
              return result;
            }}
            onCreated={(result, startNow) => {
              setCreated(result);
              setRecentIds(recordRecent(window.localStorage, result.id));
              toast.success(`${result.displayName} created`);
              if (startNow) {
                // A project created on a machine that lacks its language is the
                // most likely place to meet this offer, not the least.
                guard(startProject)(result.id)
                  .then(() => refresh())
                  .then(() => toast.success(`${result.displayName} started`))
                  .catch((error: unknown) => {
                    if (isDeclined(error)) return;
                    toast.error('Could not start the project', errorMessage(error));
                  });
              }
            }}
          />
        )}

        {paletteOpen && (
          <CommandPalette
            commands={commands}
            projects={projects ?? []}
            onOpenProject={openProjectById}
            onClose={() => setPaletteOpen(false)}
          />
        )}

        {gate}

        <UpdateManager
          open={updatesOpen}
          currentVersion={status?.appVersion ?? '—'}
          onClose={() => setUpdatesOpen(false)}
        />

        <ToastHost />
      </div>
    </>
  );
}
