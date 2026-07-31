import { progressCaption, progressPercent, type InstallPhase } from '../update';

/**
 * The bar, the byte count and the outcome, for whichever surface is showing the
 * update.
 *
 * One component because the banner and the Settings screen must not drift into
 * describing the same install differently. `tone` is the only difference: the
 * banner draws on the accent colour, Settings on the page background.
 */
export default function UpdateProgress({
  phase,
  tone,
}: {
  phase: InstallPhase;
  tone: 'banner' | 'panel';
}) {
  const percent = progressPercent(phase);
  const caption = progressCaption(phase);
  const muted = tone === 'banner' ? 'text-white/90' : 'text-neutral-300';
  const track = tone === 'banner' ? 'bg-white/25' : 'bg-neutral-700';
  const fill = tone === 'banner' ? 'bg-white' : 'bg-accent';

  if (phase.state === 'failed') {
    return (
      <p className={`mt-1.5 text-sm ${tone === 'banner' ? 'text-white/90' : 'text-amber-400'}`}>
        {phase.message}
      </p>
    );
  }

  if (phase.state === 'installed') {
    return (
      <p className={`mt-1.5 text-sm ${muted}`}>
        Installed. Close and reopen Panel Platform to finish.
      </p>
    );
  }

  if (caption === null) {
    return null;
  }

  return (
    <div className="mt-2">
      <div className={`h-1.5 w-full overflow-hidden rounded-full ${track}`}>
        <div
          className={`h-full rounded-full ${fill} ${percent === null ? 'w-1/3 animate-pulse' : 'transition-[width] duration-200'}`}
          style={percent === null ? undefined : { width: `${percent}%` }}
          role="progressbar"
          aria-valuenow={percent ?? undefined}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label="Update progress"
        />
      </div>
      <p className={`mt-1 text-xs ${muted}`}>
        {caption}
        {percent !== null && phase.state === 'downloading' ? ` — ${percent}%` : ''}
      </p>
    </div>
  );
}
