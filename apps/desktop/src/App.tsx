import { useCallback, useEffect, useState } from 'react';
import {
  createProject,
  errorMessage,
  listProjects,
  systemStatus,
  type ProjectSummary,
  type SystemStatus,
} from './api';
import Sidebar, { type View } from './components/Sidebar';
import TopBar from './components/TopBar';
import UpdateBanner from './components/UpdateBanner';
import Dashboard from './views/Dashboard';
import Projects from './views/Projects';
import ProjectConsole from './views/ProjectConsole';
import DiscordView from './views/DiscordView';
import SettingsView from './views/SettingsView';

/** How often the shell refreshes its view of the core. */
const REFRESH_MS = 5000;

export default function App() {
  const [view, setView] = useState<View>('dashboard');
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // Raised by the sidebar's own "New project" entry, handled by the projects view.
  const [createRequested, setCreateRequested] = useState(0);
  const [openProject, setOpenProject] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextProjects] = await Promise.all([systemStatus(), listProjects()]);
      setStatus(nextStatus);
      setProjects(nextProjects);
      setFailure(null);
    } catch (error) {
      setFailure(errorMessage(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  const onCreate = useCallback(
    async (name: string, description: string, runtime: string) => {
      await createProject({ displayName: name, description, runtime });
      await refresh();
    },
    [refresh],
  );

  return (
    <div className="flex h-full bg-canvas text-neutral-100">
      <Sidebar
        view={view}
        onNavigate={setView}
        projectCount={projects?.length ?? 0}
        dockerAvailable={status?.dockerAvailable ?? false}
        onNewProject={() => {
          setView('projects');
          setCreateRequested((n) => n + 1);
        }}
      />

      <div className="flex min-w-0 flex-1 flex-col">
        <UpdateBanner />
        <TopBar
          status={status}
          projectCount={projects?.length ?? 0}
          runningCount={projects?.filter((p) => p.status === 'RUNNING').length ?? 0}
        />

        <main className="flex-1 overflow-y-auto">
          {failure && (
            <div className="border-b border-red-900/60 bg-red-950/60 px-8 py-3 text-sm text-red-200">
              {failure}
            </div>
          )}

          {view === 'dashboard' && <Dashboard status={status} projects={projects} />}
          {view === 'projects' &&
            openProject !== null &&
            (() => {
              const project = projects?.find((item) => item.id === openProject);
              return project ? (
                <ProjectConsole project={project} onBack={() => setOpenProject(null)} />
              ) : null;
            })()}
          {view === 'projects' && openProject === null && (
            <Projects
              projects={projects}
              dockerAvailable={status?.dockerAvailable ?? false}
              onCreate={onCreate}
              onRefresh={refresh}
              createRequested={createRequested}
              onOpen={setOpenProject}
            />
          )}
          {view === 'discord' && <DiscordView />}
          {view === 'settings' && <SettingsView status={status} />}
        </main>
      </div>
    </div>
  );
}
