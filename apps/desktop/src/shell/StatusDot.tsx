/**
 * A project's state as a single mark.
 *
 * Never the only carrier of the state: every place this appears also shows the
 * word, because a dot that is the only difference between "running" and
 * "failed" is unreadable to anyone who cannot separate red from green.
 *
 * The pulse is reserved for a transition. A running project's dot is steady —
 * animating the normal state is what makes an interface feel restless.
 */
import { statusLook } from '../lib/projects';

const TONE: Record<string, string> = {
  ok: 'bg-ok',
  accent: 'bg-warn',
  danger: 'bg-danger',
  warn: 'bg-warn',
  neutral: 'bg-faint',
};

export default function StatusDot({ status, title }: { status: string; title?: string }) {
  const look = statusLook(status);
  return (
    <span
      aria-hidden
      title={title ?? look.label}
      className={`h-[7px] w-[7px] shrink-0 rounded-full ${TONE[look.tone] ?? 'bg-faint'} ${
        look.transitioning ? 'animate-pulse' : ''
      }`}
    />
  );
}
