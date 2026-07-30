import { useEffect, useState } from 'react';
import {
  appSettings,
  checkForUpdate,
  errorMessage,
  installUpdate,
  type AppSettings,
  type SystemStatus,
  type UpdateCheck,
} from '../api';
import PageHeader from '../components/PageHeader';
import { buttonLabel, canStart, failureMessage, idle, type InstallPhase } from '../update';

/**
 * Settings.
 *
 * Everything shown here is the configuration actually in force, read from the
 * running process. There is no write path yet, so the screen says so once at
 * the bottom rather than showing controls that would silently forget what you
 * set.
 */
export default function SettingsView({ status }: { status: SystemStatus | null }) {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [phase, setPhase] = useState<InstallPhase>(idle);

  async function install() {
    setPhase({ state: 'installing' });
    try {
      await installUpdate();
      // Linux only: on Windows the installer takes over and this process exits
      // before the await returns.
      setPhase({ state: 'installed' });
    } catch (error) {
      setPhase({ state: 'failed', message: failureMessage(error) });
    }
  }

  useEffect(() => {
    appSettings()
      .then(setSettings)
      .catch(() => setSettings(null));
  }, []);

  async function runCheck() {
    setChecking(true);
    setFailure(null);
    try {
      setCheck(await checkForUpdate());
    } catch (error) {
      // Unlike the startup banner, an explicit press deserves an answer even
      // when the answer is that the feed could not be reached.
      setFailure(errorMessage(error));
      setCheck(null);
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="px-8 py-7">
      <PageHeader
        breadcrumb="Settings"
        label="Configuration"
        title="Settings"
        subtitle="What this installation is running, and where it keeps things."
      />

      <Section title="Updates" caption="Version and release channel">
        <Row label="Installed version" value={status?.appVersion ?? '—'} />
        <Row label="Release channel" value="stable" />
        <Row label="Check on startup" value="on" />

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <button
            type="button"
            onClick={() => void runCheck()}
            disabled={checking}
            className="rounded-lg border border-edge bg-raised px-4 py-2 text-sm font-medium disabled:opacity-50"
          >
            {checking ? 'Checking…' : 'Check for updates'}
          </button>
          {check && <span className="text-sm text-neutral-300">{describe(check)}</span>}
          {failure && <span className="text-sm text-amber-400">{failure}</span>}
        </div>

        {check?.state === 'available' && (
          <div className="mt-4 rounded-lg border border-accent/40 bg-accent/10 p-4">
            <p className="text-sm font-medium">Version {check.newVersion} is available</p>
            {check.notes && <p className="mt-1.5 text-sm text-neutral-300">{check.notes}</p>}
            <button
              type="button"
              onClick={() => void install()}
              disabled={!canStart(phase)}
              className="mt-3 rounded-md bg-accent px-3 py-1.5 text-sm font-medium hover:bg-accent/90 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {buttonLabel(phase)}
            </button>
            {phase.state === 'failed' && (
              <p className="mt-2 text-sm text-amber-400">{phase.message}</p>
            )}
            {phase.state === 'installed' && (
              <p className="mt-2 text-sm text-neutral-300">
                Installed. Close and reopen Panel Platform to finish.
              </p>
            )}
          </div>
        )}
      </Section>

      <Section title="Docker" caption="Where projects actually run">
        <Row label="Status" value={status?.dockerAvailable ? 'connected' : 'unavailable'} />
        <Row label="Version" value={status?.dockerVersion ?? 'not connected'} />
        <Row label="Enabled in configuration" value={yesNo(settings?.dockerEnabled)} />
        {status && !status.dockerAvailable && status.dockerHint && (
          <p className="mt-3 text-sm text-neutral-400">{status.dockerHint}</p>
        )}
      </Section>

      <Section title="Projects" caption="Limits applied when creating and running">
        <Row label="Maximum projects" value={settings ? String(settings.maxProjects) : '—'} />
        <Row label="Largest upload" value={settings ? formatBytes(settings.maxUploadBytes) : '—'} />
        <Row
          label="Host port pool"
          value={settings ? `${settings.portPoolStart}–${settings.portPoolEnd}` : '—'}
        />
        <Row label="Ports available" value={settings ? String(settings.portPoolSize) : '—'} />
      </Section>

      <Section title="Storage" caption="Everything stays on this machine">
        <Row label="Data" value={settings?.dataDir ?? '—'} mono />
        <Row label="Projects" value={settings?.projectsDir ?? '—'} mono />
        <Row label="Logs" value={settings?.logsDir ?? '—'} mono />
        <Row label="Backups" value={settings?.backupsDir ?? '—'} mono />
        <p className="mt-3 text-sm text-neutral-400">
          Nothing leaves this machine unless you connect Discord or check for updates.
        </p>
      </Section>

      <Section title="Logging" caption="What gets written to disk">
        <Row label="Level" value={settings?.logLevel ?? '—'} />
        <Row label="Format" value={settings?.logJson ? 'structured JSON' : 'plain text'} />
        <Row label="Kept for" value={settings ? `${settings.logRetentionDays} days` : '—'} />
        <Row label="Mode" value={settings?.mode ?? '—'} />
      </Section>

      <Section title="Database" caption="Local SQLite, write-ahead logging">
        <Row label="Schema version" value={String(status?.schemaVersion ?? '—')} />
        <Row label="Started" value={status?.startedAt ?? '—'} />
      </Section>

      {/* Stated once, plainly, rather than repeated as a disabled control on
          every row above. */}
      <p className="mt-6 rounded-lg border border-edge bg-surface px-4 py-3 text-sm text-neutral-400">
        These values are read from <code className="text-neutral-300 select-text">config.toml</code>{' '}
        and <code className="text-neutral-300 select-text">PROJECT_HOST_*</code> at startup. Editing
        them from this screen is not built yet — change the file and restart.
      </p>
    </div>
  );
}

function Section({
  title,
  caption,
  children,
}: {
  title: string;
  caption: string;
  children: React.ReactNode;
}) {
  return (
    <section className="mb-4 rounded-xl border border-edge bg-surface p-5">
      <h2 className="font-medium">{title}</h2>
      <p className="mt-0.5 mb-3 text-[11px] font-semibold tracking-wider text-neutral-500 uppercase">
        {caption}
      </p>
      {children}
    </section>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-edge/60 py-2 last:border-b-0">
      <span className="shrink-0 text-sm text-neutral-400">{label}</span>
      <span
        className={`truncate text-sm text-neutral-100 ${mono ? 'font-mono text-xs select-text' : ''}`}
        title={value}
      >
        {value}
      </span>
    </div>
  );
}

function yesNo(value: boolean | undefined): string {
  if (value === undefined) return '—';
  return value ? 'yes' : 'no';
}

function formatBytes(bytes: number): string {
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return `${Math.round(gb)} GB`;
  return `${Math.round(bytes / (1024 * 1024))} MB`;
}

function describe(check: UpdateCheck): string {
  switch (check.state) {
    case 'available':
      return `Version ${check.newVersion} is available`;
    case 'up_to_date':
      return `${check.currentVersion} is up to date`;
    case 'skipped':
      return `Version ${check.skippedVersion} was skipped`;
    case 'ahead_of_published':
      return `Running ${check.currentVersion}, newer than the latest release`;
  }
}
