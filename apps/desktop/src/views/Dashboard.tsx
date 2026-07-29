import type { ProjectSummary, SystemStatus } from '../api';
import PageHeader from '../components/PageHeader';

export default function Dashboard({
  status,
  projects,
}: {
  status: SystemStatus | null;
  projects: ProjectSummary[] | null;
}) {
  const running = projects?.filter((p) => p.status === 'RUNNING').length ?? 0;
  const failed = projects?.filter((p) => p.status === 'FAILED').length ?? 0;

  return (
    <div className="px-8 py-7">
      <PageHeader
        breadcrumb="Dashboard"
        label="Overview"
        title="Dashboard"
        subtitle="Manage your projects, watch their resources, and jump into them from one place."
      />

      <div className="mt-6 grid grid-cols-2 gap-4 lg:grid-cols-4">
        <Stat label="Projects" value={projects?.length ?? 0} />
        <Stat label="Running" value={running} tone={running > 0 ? 'good' : undefined} />
        <Stat label="Failed" value={failed} tone={failed > 0 ? 'bad' : undefined} />
        <Stat label="Uptime" value={formatUptime(status?.uptimeSeconds ?? 0)} />
      </div>

      <section className="mt-6 rounded-xl border border-edge bg-surface p-5">
        <div className="flex items-center gap-3">
          <span
            className={`h-2.5 w-2.5 rounded-full ${status?.dockerAvailable ? 'bg-emerald-500' : 'bg-amber-500'}`}
            aria-hidden
          />
          <h2 className="font-medium">Docker</h2>
          <span className="text-sm text-neutral-400">{status?.dockerSummary ?? '…'}</span>
        </div>

        {/* Docker being absent is a degraded state, not a failure. Files,
            settings and backups all still work, and the wording says so
            instead of showing an alarming error. */}
        {status && !status.dockerAvailable && (
          <p className="mt-3 max-w-2xl text-sm leading-relaxed text-neutral-400">
            Projects cannot start until Docker is running. Everything else — creating projects,
            editing files and settings — still works.
            {status.dockerHint ? ` ${status.dockerHint}` : ''}
          </p>
        )}
      </section>

      <section className="mt-4 rounded-xl border border-edge bg-surface p-5">
        <h2 className="font-medium">Application</h2>
        <dl className="mt-3 grid gap-x-8 gap-y-2 text-sm sm:grid-cols-2">
          <Row label="Version" value={status?.appVersion ?? '—'} />
          <Row label="Database schema" value={String(status?.schemaVersion ?? '—')} />
          <Row label="Docker version" value={status?.dockerVersion ?? 'not connected'} />
          <Row label="Storage" value="C:\\ProgramData\\ProjectHost" />
        </dl>
      </section>
    </div>
  );
}

function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: string | number;
  tone?: 'good' | 'bad';
}) {
  const colour =
    tone === 'good' ? 'text-emerald-400' : tone === 'bad' ? 'text-red-400' : 'text-neutral-100';
  return (
    <div className="card-hover rounded-xl border border-edge bg-surface px-4 py-4">
      <p className="text-xs uppercase tracking-wide text-neutral-500">{label}</p>
      <p className={`mt-1 text-2xl font-semibold tabular-nums ${colour}`}>{value}</p>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-4 border-b border-white/5 py-1.5">
      <dt className="text-neutral-400">{label}</dt>
      <dd className="truncate text-neutral-200">{value}</dd>
    </div>
  );
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  const hours = Math.floor(seconds / 3600);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}
