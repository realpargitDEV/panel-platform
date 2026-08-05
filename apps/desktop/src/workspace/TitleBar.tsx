/**
 * The strip along the top: navigation, the workspace name, the command field,
 * and the layout toggles.
 *
 * There are no window controls here. Tauri draws the real ones — this window
 * has its native decorations — and a second, painted set that does not minimise
 * anything is exactly the kind of thing that makes an application feel like a
 * web page in a frame.
 */
import Icon from './Icon';
import MenuBar, { type Menu } from './MenuBar';

export default function TitleBar({
  menus,
  projectName,
  canGoBack,
  onBack,
  onForward,
  canGoForward,
  onOpenPalette,
  update,
  sidebarVisible,
  panelVisible,
  onToggleSidebar,
  onTogglePanel,
}: {
  menus: Menu[];
  projectName: string;
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
  onOpenPalette: () => void;
  /** Present only when a release is actually offered. */
  /** An available update, and the way to the window that installs it. This is
   *  an entry point, not a progress display: the update manager reports the
   *  install, so the title bar has no state of its own to keep in step. */
  update: { label: string; onOpen: () => void } | null;
  sidebarVisible: boolean;
  panelVisible: boolean;
  onToggleSidebar: () => void;
  onTogglePanel: () => void;
}) {
  return (
    <header className="flex h-9 shrink-0 items-stretch gap-1 border-b border-vs-border bg-vs-titlebar px-1.5">
      <div className="flex items-center">
        <MenuBar menus={menus} />
      </div>

      <div className="ml-2 flex items-center gap-0.5">
        <TitleButton icon="arrow-left" label="Go back" disabled={!canGoBack} onClick={onBack} />
        <TitleButton
          icon="arrow-right"
          label="Go forward"
          disabled={!canGoForward}
          onClick={onForward}
        />
      </div>

      {/* The command field is centred on the *window*, not on the space left
          over, which is why it is absolutely positioned rather than flexed. */}
      <div className="relative flex flex-1 items-center justify-center">
        <button
          type="button"
          onClick={onOpenPalette}
          title="Search files and run commands (Ctrl+P)"
          className="flex h-[22px] w-full max-w-[560px] min-w-0 items-center gap-1.5 rounded-[3px] border border-vs-border bg-vs-editor px-2 text-[12px] text-vs-dim hover:border-white/20 hover:bg-white/5"
        >
          <Icon name="search" size={13} />
          <span className="truncate">{projectName}</span>
        </button>
      </div>

      <div className="flex items-center gap-1">
        {update && (
          <button
            type="button"
            onClick={update.onOpen}
            title="Open the update manager"
            className="flex items-center gap-1.5 rounded-[3px] bg-accent px-2 py-0.5 text-[12px] font-medium text-white hover:brightness-110"
          >
            <Icon name="refresh" size={13} />
            {update.label}
          </button>
        )}
        <TitleButton
          icon="sidebar"
          label={sidebarVisible ? 'Hide the side bar (Ctrl+B)' : 'Show the side bar (Ctrl+B)'}
          active={sidebarVisible}
          onClick={onToggleSidebar}
        />
        <TitleButton
          icon="panel"
          label={panelVisible ? 'Hide the panel (Ctrl+J)' : 'Show the panel (Ctrl+J)'}
          active={panelVisible}
          onClick={onTogglePanel}
        />
      </div>
    </header>
  );
}

function TitleButton({
  icon,
  label,
  onClick,
  disabled,
  active,
}: {
  icon: Parameters<typeof Icon>[0]['name'];
  label: string;
  onClick: () => void;
  disabled?: boolean;
  active?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
      className={`grid h-6 w-7 place-items-center rounded-[3px] text-vs-text hover:bg-white/10 disabled:text-vs-dim disabled:opacity-40 disabled:hover:bg-transparent ${
        active ? 'text-white' : ''
      }`}
    >
      <Icon name={icon} size={16} />
    </button>
  );
}
