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
                    step.state === 'ready' ? 'bg-ok-soft text-ok' : 'border border-edge text-faint'
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
    </PageShell>
  );
}
