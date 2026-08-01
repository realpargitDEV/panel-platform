/**
 * The bar above the content.
 *
 * It carries the things that are true everywhere: how to search, whether the
 * machine is healthy, whether anything needs attention, and the application
 * menu. The project counts that used to live here are gone — they were the same
 * four numbers the dashboard shows a hundred pixels below, and a statistic
 * repeated twice is read as two different statistics.
 */
import { useState } from 'react';

import Icon from '../ui/Icon';
import { Menu, useMenu } from '../ui/overlays';
import { Badge, IconButton } from '../ui/primitives';

export default function TopBar({
  version,
  runningCount,
  attentionCount,
  dockerAvailable,
  updateAvailable,
  onOpenPalette,
  onOpenSettings,
  onCheckUpdates,
  onOpenActivity,
  onInstallUpdate,
}: {
  version: string;
  runningCount: number;
  attentionCount: number;
  dockerAvailable: boolean;
  updateAvailable: string | null;
  onOpenPalette: () => void;
  onOpenSettings: () => void;
  onCheckUpdates: () => void;
  onOpenActivity: () => void;
  onInstallUpdate: () => void;
}) {
  const menu = useMenu();
  const [platform] = useState(() =>
    typeof navigator === 'undefined' ? '' : navigator.userAgent.toLowerCase(),
  );
  const modifier = platform.includes('mac') ? '⌘' : 'Ctrl';

  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-edge bg-surface px-4">
      {/* The search field is a button: it opens the palette rather than
          filtering in place, so there is one search in the application. */}
      <button
        type="button"
        onClick={onOpenPalette}
        className="flex h-8 w-full max-w-[420px] items-center gap-2 rounded-[8px] border border-edge bg-canvas px-2.5 text-[13px] text-faint hover:border-edge-strong hover:text-muted"
      >
        <Icon name="search" size={14} />
        <span className="flex-1 text-left">Search projects and commands</span>
        <kbd className="rounded-[4px] border border-edge px-1.5 py-0.5 text-[11px] text-faint">
          {modifier} K
        </kbd>
      </button>

      <span className="flex-1" />

      {updateAvailable && (
        <button
          type="button"
          onClick={onInstallUpdate}
          className="hidden items-center gap-1.5 rounded-full bg-accent-soft px-2.5 py-1 text-[12px] font-medium text-accent hover:bg-accent/20 sm:inline-flex"
        >
          <Icon name="download" size={13} />
          Update {updateAvailable}
        </button>
      )}

      {attentionCount > 0 && (
        <button
          type="button"
          onClick={onOpenActivity}
          title={`${attentionCount} project${attentionCount === 1 ? '' : 's'} need attention`}
          className="rounded-full"
        >
          <Badge tone="warn" dot>
            {attentionCount} need{attentionCount === 1 ? 's' : ''} attention
          </Badge>
        </button>
      )}

      <span
        title={dockerAvailable ? 'Docker responded' : 'Docker is not available'}
        className="hidden items-center gap-1.5 text-[12px] text-muted md:flex"
      >
        <span
          aria-hidden
          className={`h-2 w-2 rounded-full ${dockerAvailable ? 'bg-ok' : 'bg-warn'}`}
        />
        <span className="tabular">{runningCount} running</span>
      </span>

      <IconButton icon="bell" label="Activity" onClick={onOpenActivity} />

      <button
        type="button"
        onClick={menu.open}
        aria-haspopup="menu"
        aria-label="Application menu"
        className="grid h-8 w-8 shrink-0 place-items-center rounded-full border border-edge bg-raised text-muted hover:border-edge-strong hover:text-ink"
      >
        <Icon name="user" size={15} />
      </button>

      {menu.anchor && (
        <Menu
          anchor={menu.anchor}
          onClose={menu.close}
          items={[
            {
              id: 'version',
              label: `Version ${version}`,
              icon: 'info',
              disabled: true,
              run: () => {},
            },
            { id: 'updates', label: 'Check for updates', icon: 'download', run: onCheckUpdates },
            { id: 'settings', label: 'Settings', icon: 'settings', run: onOpenSettings },
            { id: 'activity', label: 'Activity log', icon: 'activity', run: onOpenActivity },
          ]}
        />
      )}
    </header>
  );
}
