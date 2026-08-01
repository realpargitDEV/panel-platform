/**
 * Settings.
 *
 * Grouped into tabs, with real controls where a control does something and a
 * plain reading where it does not. The core has no command that writes its
 * configuration — it is loaded from `config.toml` and the environment at
 * startup — so most of this screen reports rather than edits.
 *
 * Rather than dress that up with switches that forget what you set, the values
 * that cannot change are read-only rows, the ones that can (this window's own
 * preferences, the update check, opening a folder) are real controls, and the
 * screen says which is which once, at the top of General.
 */
import { useEffect, useState } from 'react';

import {
  appSettings,
  errorMessage,
  revealProjectPath,
  systemMetrics,
  type AppSettings,
  type ProjectSummary,
  type SystemMetrics,
  type SystemStatus,
} from '../api';
import { formatBytes, formatDuration } from '../lib/format';
import { buttonLabel, canStart, describeCheck } from '../update';
import { updateStore, useUpdate } from '../useUpdate';
import Icon from '../ui/Icon';
import { ConfirmDialog } from '../ui/overlays';
import UpdateProgress from '../components/UpdateProgress';
import {
  Badge,
  Button,
  Card,
  CardHeader,
  DataRow,
  PageShell,
  Skeleton,
  Tabs,
  Toggle,
} from '../ui/primitives';
import { toast } from '../ui/toast';

type TabId = 'general' | 'docker' | 'storage' | 'networking' | 'updates' | 'logging' | 'about';

/** This window's own preferences — the settings that genuinely are writable. */
export interface Preferences {
  collapsedSidebar: boolean;
  confirmDestructive: boolean;
}

export default function Settings({
  status,
  projects,
  preferences,
  onPreferences,
  onResetLayout,
}: {
  status: SystemStatus | null;
  projects: ProjectSummary[] | null;
  preferences: Preferences;
  onPreferences: (next: Partial<Preferences>) => void;
  onResetLayout: () => void;
}) {
  const [tab, setTab] = useState<TabId>('general');
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const { check, checking, checkFailure, phase } = useUpdate();

  useEffect(() => {
    appSettings()
      .then(setSettings)
      .catch((error: unknown) => toast.error('Could not read the settings', errorMessage(error)));
    systemMetrics()
      .then(setMetrics)
      .catch(() => setMetrics(null));
  }, []);

  function copy(value: string, what: string) {
    navigator.clipboard
      .writeText(value)
      .then(() => toast.success(`${what} copied`))
      .catch((error: unknown) => toast.error('Could not copy', errorMessage(error)));
  }

  /**
   * Opening a folder goes through the project reveal command, which resolves
   * inside a project root. Only a project's own folder can be opened this way,
   * so the data directory offers a copy button instead of a broken one.
   */
  function openProjectFolder() {
    const first = projects?.[0];
    if (!first) {
      toast.info('No project to open', 'Create a project first.');
      return;
    }
    revealProjectPath(first.id, '').catch((error: unknown) =>
      toast.error('Could not open the folder', errorMessage(error)),
    );
  }

  const pathRow = (label: string, value: string | undefined) => (
    <div className="flex items-center gap-2 border-b border-edge/60 py-2 last:border-b-0">
      <span className="shrink-0 text-[13px] text-muted">{label}</span>
      <span className="min-w-0 flex-1 truncate text-right font-mono text-[12px] text-ink select-text">
        {value ?? '—'}
      </span>
      <Button
        size="sm"
        variant="ghost"
        icon="copy"
        disabled={!value}
        title={`Copy the ${label.toLowerCase()}`}
        onClick={() => value && copy(value, label)}
      />
    </div>
  );

  return (
    <PageShell title="Settings" description="What this installation is running, and where.">
      <Tabs<TabId>
        active={tab}
        onSelect={setTab}
        tabs={[
          { id: 'general', label: 'General' },
          { id: 'docker', label: 'Docker' },
          { id: 'storage', label: 'Storage' },
          { id: 'networking', label: 'Networking' },
          { id: 'updates', label: 'Updates' },
          { id: 'logging', label: 'Logging' },
          { id: 'about', label: 'System information' },
        ]}
      />

      <div className="pt-4">
        {tab === 'general' && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader title="This window" subtitle="Preferences stored on this machine" />
              <div className="px-4 py-1">
                <Toggle
                  checked={preferences.collapsedSidebar}
                  onChange={(collapsedSidebar) => onPreferences({ collapsedSidebar })}
                  label="Collapse the sidebar"
                  description="Show icons only. Ctrl+B toggles it at any time."
                />
                <Toggle
                  checked={preferences.confirmDestructive}
                  onChange={(confirmDestructive) => onPreferences({ confirmDestructive })}
                  label="Confirm destructive actions"
                  description="Ask before force-killing a project or deleting a file."
                />
              </div>
              <div className="border-t border-edge px-4 py-3">
                <Button icon="restart" onClick={() => setConfirmReset(true)}>
                  Reset layout and preferences
                </Button>
              </div>
            </Card>

            <Card>
              <CardHeader title="Configuration" />
              <div className="px-4 py-3">
                <p className="text-[13px] leading-relaxed text-muted">
                  Everything outside this card is read from{' '}
                  <code className="rounded-[4px] bg-canvas px-1 py-0.5 font-mono text-[12px] text-ink select-text">
                    config.toml
                  </code>{' '}
                  and{' '}
                  <code className="rounded-[4px] bg-canvas px-1 py-0.5 font-mono text-[12px] text-ink select-text">
                    PROJECT_HOST_*
                  </code>{' '}
                  when the application starts. The core has no command that writes them back, so
                  those screens report rather than edit — change the file and restart.
                </p>
              </div>
              <div className="border-t border-edge px-4 py-1">
                <DataRow label="Mode" value={settings?.mode ?? '—'} />
                <DataRow label="Maximum projects" value={settings ? settings.maxProjects : '—'} />
                <DataRow
                  label="Largest upload"
                  value={settings ? formatBytes(settings.maxUploadBytes) : '—'}
                />
              </div>
            </Card>
          </div>
        )}

        {tab === 'docker' && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader
                title="Connection"
                actions={
                  <Button
                    size="sm"
                    icon="refresh"
                    onClick={() => {
                      // The shell polls system status every few seconds, so a
                      // retry is a re-read rather than a special command.
                      toast.info('Rechecking Docker', 'The status refreshes within a few seconds.');
                    }}
                  >
                    Retry
                  </Button>
                }
              />
              <div className="px-4 py-1">
                <DataRow
                  label="Status"
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
                <DataRow label="Version" value={status?.dockerVersion ?? 'not connected'} />
                <DataRow label="Detail" value={status?.dockerSummary ?? '—'} />
                <DataRow
                  label="Enabled in configuration"
                  value={settings ? (settings.dockerEnabled ? 'yes' : 'no') : '—'}
                />
              </div>
              {status && !status.dockerAvailable && (
                <div className="mx-4 mb-4 rounded-[8px] border border-warn/30 bg-warn-soft px-3 py-2.5">
                  <p className="text-[12px] font-medium text-warn">Projects cannot start</p>
                  <p className="mt-0.5 text-[12px] text-muted">
                    {status.dockerHint ??
                      'Install Docker and start it, then this page will pick it up.'}
                  </p>
                </div>
              )}
            </Card>

            <Card>
              <CardHeader title="What still works without Docker" />
              <ul className="px-4 py-2">
                {[
                  'Creating projects, including cloning a repository',
                  'Editing, uploading and organising project files',
                  'Reading settings, activity and project history',
                ].map((item) => (
                  <li
                    key={item}
                    className="flex items-start gap-2 border-b border-edge/60 py-2 text-[13px] text-muted last:border-b-0"
                  >
                    <span className="mt-0.5 text-ok">
                      <Icon name="check" size={14} />
                    </span>
                    {item}
                  </li>
                ))}
              </ul>
            </Card>
          </div>
        )}

        {tab === 'storage' && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader
                title="Locations"
                actions={
                  <Button size="sm" icon="external" onClick={openProjectFolder}>
                    Open a project folder
                  </Button>
                }
              />
              <div className="px-4 py-1">
                {pathRow('Data', settings?.dataDir)}
                {pathRow('Projects', settings?.projectsDir)}
                {pathRow('Logs', settings?.logsDir)}
                {pathRow('Backups', settings?.backupsDir)}
              </div>
              <p className="border-t border-edge px-4 py-2.5 text-[12px] text-muted">
                Nothing leaves this machine unless you check for updates.
              </p>
            </Card>

            <Card>
              <CardHeader title="Disk" subtitle={metrics?.diskMount} />
              {metrics === null ? (
                <div className="p-4">
                  <Skeleton className="h-16" />
                </div>
              ) : (
                <div className="px-4 py-1">
                  <DataRow label="Used" value={formatBytes(metrics.diskUsedBytes)} />
                  <DataRow label="Total" value={formatBytes(metrics.diskTotalBytes)} />
                  <DataRow
                    label="Free"
                    value={formatBytes(metrics.diskTotalBytes - metrics.diskUsedBytes)}
                  />
                </div>
              )}
              <p className="border-t border-edge px-4 py-2.5 text-[12px] text-muted">
                Backups are not built yet. The folder above is reserved for them.
              </p>
            </Card>
          </div>
        )}

        {tab === 'networking' && (
          <Card className="max-w-xl">
            <CardHeader
              title="Host port pool"
              subtitle="Ports projects are given on this machine"
            />
            <div className="px-4 py-1">
              <DataRow
                label="Range"
                value={settings ? `${settings.portPoolStart}–${settings.portPoolEnd}` : '—'}
              />
              <DataRow label="Ports available" value={settings ? settings.portPoolSize : '—'} />
            </div>
            <p className="border-t border-edge px-4 py-2.5 text-[12px] text-muted">
              A project is allocated a free port from this range when it first starts. The port it
              received is on the project&apos;s Networking tab.
            </p>
          </Card>
        )}

        {tab === 'updates' && (
          <Card className="max-w-xl">
            <CardHeader
              title="Updates"
              actions={
                <Button
                  size="sm"
                  icon="refresh"
                  disabled={checking || !canStart(phase)}
                  onClick={() => void updateStore.check()}
                >
                  {checking ? 'Checking…' : 'Check now'}
                </Button>
              }
            />
            <div className="px-4 py-1">
              <DataRow label="Installed version" value={status?.appVersion ?? '—'} />
              <DataRow label="Release channel" value="stable" />
              <DataRow label="Check on startup" value="on" />
              <DataRow label="Check while running" value="every 6 hours" />
            </div>

            {check && <p className="px-4 py-2 text-[13px] text-muted">{describeCheck(check)}</p>}
            {checkFailure && <p className="px-4 py-2 text-[13px] text-warn">{checkFailure}</p>}

            {check?.state === 'available' && (
              <div className="mx-4 mb-4 rounded-[10px] border border-accent/40 bg-accent-soft p-3">
                <div className="flex flex-wrap items-center gap-3">
                  <p className="flex-1 text-[13px] font-medium text-ink">
                    Version {check.newVersion} is available
                  </p>
                  <Button
                    variant="primary"
                    size="sm"
                    disabled={!canStart(phase)}
                    onClick={() => void updateStore.install()}
                  >
                    {buttonLabel(phase)}
                  </Button>
                </div>
                {check.notes && <p className="mt-1.5 text-[12px] text-muted">{check.notes}</p>}
                <UpdateProgress phase={phase} tone="panel" />
              </div>
            )}

            <p className="border-t border-edge px-4 py-2.5 text-[12px] text-muted">
              The release channel is fixed to stable in this build.
            </p>
          </Card>
        )}

        {tab === 'logging' && (
          <Card className="max-w-xl">
            <CardHeader title="Logging" subtitle="What the core writes to disk" />
            <div className="px-4 py-1">
              <DataRow label="Level" value={settings?.logLevel ?? '—'} />
              <DataRow
                label="Format"
                value={settings ? (settings.logJson ? 'structured JSON' : 'plain text') : '—'}
              />
              <DataRow
                label="Kept for"
                value={settings ? `${settings.logRetentionDays} days` : '—'}
              />
              {pathRow('Directory', settings?.logsDir)}
            </div>
            <p className="border-t border-edge px-4 py-2.5 text-[12px] text-muted">
              Reading those files back into this window is not built yet, so nothing is shown here
              that has not been read.
            </p>
          </Card>
        )}

        {tab === 'about' && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader title="Application" />
              <div className="px-4 py-1">
                <DataRow label="Version" value={status?.appVersion ?? '—'} />
                <DataRow label="Database schema" value={status?.schemaVersion ?? '—'} />
                <DataRow
                  label="Uptime"
                  value={status ? formatDuration(status.uptimeSeconds) : '—'}
                />
                <DataRow label="Started" value={status?.startedAt ?? '—'} />
              </div>
            </Card>

            <Card>
              <CardHeader title="Machine" />
              {metrics === null ? (
                <div className="p-4">
                  <Skeleton className="h-24" />
                </div>
              ) : (
                <div className="px-4 py-1">
                  <DataRow label="Logical cores" value={metrics.cpuCount} />
                  <DataRow label="Memory" value={formatBytes(metrics.memoryTotalBytes)} />
                  <DataRow label="Projects volume" value={metrics.diskMount || '—'} mono />
                </div>
              )}
              <div className="border-t border-edge px-4 py-3">
                <Button
                  icon="copy"
                  onClick={() =>
                    copy(JSON.stringify({ status, settings, metrics }, null, 2), 'Diagnostics')
                  }
                >
                  Copy diagnostics
                </Button>
              </div>
            </Card>
          </div>
        )}
      </div>

      {confirmReset && (
        <ConfirmDialog
          title="Reset layout and preferences?"
          description="Panel sizes, the sidebar state and this window's preferences return to their defaults. Projects and files are untouched."
          confirmLabel="Reset"
          onCancel={() => setConfirmReset(false)}
          onConfirm={() => {
            setConfirmReset(false);
            onResetLayout();
            toast.success('Preferences reset');
          }}
        />
      )}
    </PageShell>
  );
}
