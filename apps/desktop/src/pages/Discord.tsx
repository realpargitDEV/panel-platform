/**
 * The Discord integration.
 *
 * The honest shape of this screen is a setup flow with its first step blocked.
 * The permission model, the control panel, the channel naming and the message
 * safety rules are built and tested in the core; the gateway connection that
 * carries them to Discord is not written, so no server can be linked.
 *
 * Rather than show a Connect button that fails, the flow shows where it stops:
 * the steps that are ready are ticked, the one that is not says so, and the
 * button explains itself instead of pretending.
 */
import Icon, { type IconName } from '../ui/Icon';
import { Badge, Button, Card, CardHeader, PageShell } from '../ui/primitives';

interface Step {
  title: string;
  detail: string;
  state: 'ready' | 'blocked';
}

const STEPS: Step[] = [
  {
    title: 'Permission model',
    detail: 'Who may start, stop and inspect a project, by Discord role.',
    state: 'ready',
  },
  {
    title: 'Control panel and channels',
    detail: 'A log channel and a control channel per project, named consistently.',
    state: 'ready',
  },
  {
    title: 'Message safety',
    detail: 'Secrets stripped and mentions neutralised before anything is posted.',
    state: 'ready',
  },
  {
    title: 'Gateway connection',
    detail: 'The live connection to Discord that carries all of the above. Not written yet.',
    state: 'blocked',
  },
];

const FEATURES: { icon: IconName; title: string; detail: string }[] = [
  {
    icon: 'logs',
    title: 'Per-project channels',
    detail: 'Every project gets a log channel and a control panel channel.',
  },
  {
    icon: 'play',
    title: 'Control from Discord',
    detail: 'Start, stop, restart and inspect a project from buttons in a message.',
  },
  {
    icon: 'shield',
    title: 'Role-based access',
    detail: 'Restrict who can do what by Discord role, with everything audited.',
  },
  {
    icon: 'alert',
    title: 'Safe output',
    detail: 'Secrets are stripped and mentions neutralised before posting.',
  },
];

const PERMISSIONS = [
  'Manage Channels — to create the log and control channels',
  'Send Messages — to post status and control panels',
  'Embed Links — for the control panel layout',
  'Read Message History — to update a panel it posted earlier',
];

export default function Discord() {
  const ready = STEPS.filter((step) => step.state === 'ready').length;

  return (
    <PageShell title="Discord" description="Watch and control projects from a Discord server.">
      {/* Everything below opts out of the user's theme. Discord's surface is
          Discord's identity, so this panel stays Discord-coloured on Light,
          Amber and Nord alike — see `.discord-scope` in `styles.css`. */}
      <div className="discord-scope animate-view rounded-card">
        <DiscordPreview />

        <Card className="mb-4">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-3 px-4 py-3.5">
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-[10px] border border-edge bg-raised text-muted">
              <Icon name="discord" size={18} />
            </span>
            <div className="min-w-[220px] flex-1">
              <div className="flex items-center gap-2">
                <p className="text-[14px] font-medium text-ink">Not connected</p>
                <Badge tone="neutral" dot>
                  {ready} of {STEPS.length} ready
                </Badge>
              </div>
              <p className="mt-0.5 text-[13px] text-muted">
                The connection to Discord is not built yet, so no server can be linked from here.
              </p>
            </div>
            <Button disabled title="The gateway connection is not implemented yet" icon="discord">
              Connect a server
            </Button>
          </div>
        </Card>

        <div className="grid gap-4 lg:grid-cols-[1fr_320px]">
          <Card>
            <CardHeader title="Setup" subtitle="What is built, and what is missing" />
            <ol className="px-4 py-2">
              {STEPS.map((step, index) => (
                <li
                  key={step.title}
                  className="flex items-start gap-3 border-b border-edge/60 py-2.5 last:border-b-0"
                >
                  <span
                    className={`mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-full text-[11px] font-semibold ${
                      step.state === 'ready'
                        ? 'bg-ok-soft text-ok'
                        : 'border border-edge text-faint'
                    }`}
                  >
                    {step.state === 'ready' ? <Icon name="check" size={12} /> : index + 1}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex flex-wrap items-center gap-2">
                      <span className="text-[13px] text-ink">{step.title}</span>
                      {step.state === 'blocked' && <Badge tone="warn">not built</Badge>}
                    </span>
                    <span className="mt-0.5 block text-[12px] text-muted">{step.detail}</span>
                  </span>
                </li>
              ))}
            </ol>
          </Card>

          <Card>
            <CardHeader title="Permissions it will ask for" />
            <ul className="px-4 py-2">
              {PERMISSIONS.map((permission) => (
                <li
                  key={permission}
                  className="flex items-start gap-2 border-b border-edge/60 py-2 text-[12px] text-muted last:border-b-0"
                >
                  <span className="mt-0.5 text-faint">
                    <Icon name="shield" size={13} />
                  </span>
                  {permission}
                </li>
              ))}
            </ul>
          </Card>
        </div>

        <Card className="mt-4">
          <CardHeader title="What it will do" />
          <div className="grid gap-px bg-edge sm:grid-cols-2">
            {FEATURES.map((feature) => (
              <div key={feature.title} className="flex items-start gap-3 bg-surface px-4 py-3.5">
                <span className="mt-0.5 text-muted">
                  <Icon name={feature.icon} size={16} />
                </span>
                <div className="min-w-0">
                  <p className="text-[13px] text-ink">{feature.title}</p>
                  <p className="mt-0.5 text-[12px] text-muted">{feature.detail}</p>
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </PageShell>
  );
}

// ------------------------------------------------------------------ preview

const SERVERS = [
  { id: 'pp', label: 'PP', tone: '#5865f2', active: true },
  { id: 'ops', label: 'OP', tone: '#23a559', active: false },
  { id: 'dev', label: 'DV', tone: '#eb459e', active: false },
];

const CHANNELS = [
  { name: 'panel-control', kind: 'control' as const, active: true },
  { name: 'api-logs', kind: 'log' as const, active: false },
  { name: 'bot-logs', kind: 'log' as const, active: false },
  { name: 'site-logs', kind: 'log' as const, active: false },
];

/**
 * What a linked server will look like.
 *
 * Explicitly a mock-up, and labelled as one on the surface rather than only in
 * this comment: the gateway is not written, so there is no server to read and
 * anything here that looked live would be a lie. The shapes are Discord's —
 * the server rail with its squircle pills, the `#` channel list, an embed with
 * a control row — so that what is being promised is legible at a glance.
 */
function DiscordPreview() {
  return (
    <Card className="mb-4 overflow-hidden">
      <CardHeader
        title="What a linked server looks like"
        subtitle="A preview, not a connection — nothing here is live"
      />

      <div className="flex h-[260px] min-h-0 overflow-hidden border-t border-edge text-[13px]">
        {/* Server rail */}
        <div className="flex w-[68px] shrink-0 flex-col items-center gap-2 bg-canvas py-3">
          {SERVERS.map((server) => (
            <span
              key={server.id}
              aria-hidden
              className={`grid h-10 w-10 place-items-center text-[12px] font-semibold text-white transition-[border-radius] duration-200 ${
                server.active ? 'rounded-[14px]' : 'rounded-[20px] hover:rounded-[14px]'
              }`}
              style={{ background: server.tone }}
            >
              {server.label}
            </span>
          ))}
        </div>

        {/* Channels */}
        <div className="w-[168px] shrink-0 bg-surface py-3">
          <p className="px-3 pb-2 text-[11px] font-semibold tracking-wide text-faint uppercase">
            Projects
          </p>
          <ul className="stagger px-2">
            {CHANNELS.map((channel) => (
              <li key={channel.name}>
                <span
                  className={`flex h-8 items-center gap-1.5 rounded-[4px] px-2 ${
                    channel.active ? 'bg-overlay text-ink' : 'text-muted'
                  }`}
                >
                  <span aria-hidden className="text-faint">
                    #
                  </span>
                  <span className="truncate">{channel.name}</span>
                </span>
              </li>
            ))}
          </ul>
        </div>

        {/* A control panel message */}
        <div className="min-w-0 flex-1 bg-raised p-4">
          <div className="flex gap-3">
            <span
              aria-hidden
              className="grid h-10 w-10 shrink-0 place-items-center rounded-full text-white"
              style={{ background: '#5865f2' }}
            >
              <Icon name="container" size={18} />
            </span>
            <div className="min-w-0 flex-1">
              <p className="flex items-baseline gap-2">
                <span className="font-medium text-ink">Panel Platform</span>
                <span
                  className="rounded-[3px] px-1 text-[10px] font-semibold text-white"
                  style={{ background: '#5865f2' }}
                >
                  APP
                </span>
              </p>

              <div className="mt-1 rounded-[4px] border-l-4 border-l-ok bg-surface p-3">
                <p className="font-medium text-ink">api · running</p>
                <div className="mt-2 grid grid-cols-2 gap-y-1 text-[12px] text-muted">
                  <span>Uptime</span>
                  <span className="tabular text-ink">4h 12m</span>
                  <span>Restarts</span>
                  <span className="tabular text-ink">0</span>
                </div>
                <div className="mt-3 flex flex-wrap gap-2">
                  <span className="rounded-[3px] bg-overlay px-3 py-1.5 text-[12px] font-medium text-ink">
                    Restart
                  </span>
                  <span className="rounded-[3px] bg-overlay px-3 py-1.5 text-[12px] font-medium text-ink">
                    Stop
                  </span>
                  <span
                    className="rounded-[3px] px-3 py-1.5 text-[12px] font-medium text-white"
                    style={{ background: '#5865f2' }}
                  >
                    Logs
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Card>
  );
}
