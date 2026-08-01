/**
 * The Explorer sidebar: a title, a toolbar, the tree, and the transfers that
 * are running.
 *
 * The old "Upload target" box is gone. Where a drop lands is shown on the row
 * that will receive it — highlighted while a drag is in progress — and stated
 * once in the drop overlay, rather than occupying eighty pixels of the sidebar
 * permanently to describe something that only matters mid-drag.
 */
import { forwardRef } from 'react';

import Icon from './Icon';
import FileTree, { type TreeCallbacks, type TreeState } from './FileTree';
import type { UploadItem, UploadStatus } from './uploads';

export interface ExplorerActions {
  onNewFile: () => void;
  onNewFolder: () => void;
  onRefresh: () => void;
  onCollapseAll: () => void;
}

const ExplorerPanel = forwardRef<
  HTMLDivElement,
  {
    projectName: string;
    state: TreeState;
    callbacks: TreeCallbacks;
    actions: ExplorerActions;
    uploads: UploadItem[];
    draggingOver: boolean;
    /** Where a drop would land, phrased for a person. */
    dropTargetLabel: string;
    onSubmitEdit: (value: string) => void;
    onCancelEdit: () => void;
    onCancelUpload: (item: UploadItem) => void;
    onRetryUpload: (item: UploadItem) => void;
    onClearFinishedUploads: () => void;
    onEmptyAreaClick: () => void;
    onEmptyAreaContextMenu: (event: React.MouseEvent) => void;
    onDragEnter: (event: React.DragEvent<HTMLElement>) => void;
    onDragLeave: (event: React.DragEvent<HTMLElement>) => void;
    onDragOver: (event: React.DragEvent<HTMLElement>) => void;
    onDrop: (event: React.DragEvent<HTMLElement>) => void;
  }
>(function ExplorerPanel(
  {
    projectName,
    state,
    callbacks,
    actions,
    uploads,
    draggingOver,
    dropTargetLabel,
    onSubmitEdit,
    onCancelEdit,
    onCancelUpload,
    onRetryUpload,
    onClearFinishedUploads,
    onEmptyAreaClick,
    onEmptyAreaContextMenu,
    onDragEnter,
    onDragLeave,
    onDragOver,
    onDrop,
  },
  ref,
) {
  return (
    <div
      ref={ref}
      onDragEnter={onDragEnter}
      onDragLeave={onDragLeave}
      onDragOver={onDragOver}
      onDrop={onDrop}
      className="relative flex min-h-0 flex-1 flex-col"
    >
      <div className="flex h-9 shrink-0 items-center px-5 text-[11px] font-normal tracking-wide text-vs-dim uppercase">
        Explorer
      </div>

      <div className="group/section flex h-[22px] shrink-0 items-center gap-1 bg-white/[0.03] pr-1 pl-2">
        <Icon name="chevron-down" size={14} />
        <span className="flex-1 truncate text-[11px] font-bold tracking-wide uppercase">
          {projectName}
        </span>
        <ToolbarButton icon="new-file" label="New File… (Ctrl+N)" onClick={actions.onNewFile} />
        <ToolbarButton icon="new-folder" label="New Folder…" onClick={actions.onNewFolder} />
        <ToolbarButton icon="refresh" label="Refresh Explorer" onClick={actions.onRefresh} />
        <ToolbarButton
          icon="collapse-all"
          label="Collapse Folders in Explorer"
          onClick={actions.onCollapseAll}
        />
      </div>

      {/* The click and context-menu handlers are on the scroll area so the
          blank space below the tree acts on the project root, as it does in
          VS Code. Rows stop the event themselves. */}
      <div
        onClick={onEmptyAreaClick}
        onContextMenu={onEmptyAreaContextMenu}
        className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden py-0.5"
      >
        <FileTree
          directory=""
          depth={0}
          state={state}
          callbacks={callbacks}
          onSubmitEdit={onSubmitEdit}
          onCancelEdit={onCancelEdit}
        />
      </div>

      {uploads.length > 0 && (
        <Transfers
          uploads={uploads}
          onCancel={onCancelUpload}
          onRetry={onRetryUpload}
          onClearFinished={onClearFinishedUploads}
        />
      )}

      {draggingOver && (
        <div className="pointer-events-none absolute inset-0 z-20 flex items-end justify-center bg-accent/10 p-3 ring-1 ring-accent ring-inset">
          <p className="rounded-[3px] bg-[#12182a] px-2.5 py-1.5 text-[12px] text-vs-text shadow-lg">
            Drop into <span className="font-mono text-white">{dropTargetLabel}</span>
          </p>
        </div>
      )}
    </div>
  );
});

export default ExplorerPanel;

function ToolbarButton({
  icon,
  label,
  onClick,
}: {
  icon: Parameters<typeof Icon>[0]['name'];
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
      // Hidden until the section is hovered or something inside it has focus,
      // the way VS Code hides its section actions.
      className="grid h-5 w-5 place-items-center rounded-[3px] text-vs-text opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100 hover:bg-white/10"
    >
      <Icon name={icon} size={15} />
    </button>
  );
}

/**
 * Running and finished transfers.
 *
 * Every number here comes from the core's own progress events; nothing is
 * estimated. An import whose total is not known yet shows an indeterminate bar
 * rather than a percentage nobody computed.
 */
function Transfers({
  uploads,
  onCancel,
  onRetry,
  onClearFinished,
}: {
  uploads: UploadItem[];
  onCancel: (item: UploadItem) => void;
  onRetry: (item: UploadItem) => void;
  onClearFinished: () => void;
}) {
  const finished = uploads.some(
    (item) => item.status === 'success' || item.status === 'failed' || item.status === 'cancelled',
  );

  return (
    <section className="shrink-0 border-t border-vs-border">
      <div className="group/section flex h-[22px] items-center gap-1 bg-white/[0.03] pr-1 pl-2">
        <Icon name="chevron-down" size={14} />
        <span className="flex-1 text-[11px] font-bold tracking-wide uppercase">Transfers</span>
        {finished && (
          <button
            type="button"
            onClick={onClearFinished}
            title="Clear finished transfers"
            aria-label="Clear finished transfers"
            className="grid h-5 w-5 place-items-center rounded-[3px] text-vs-text opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100 hover:bg-white/10"
          >
            <Icon name="close" size={14} />
          </button>
        )}
      </div>

      <ul className="max-h-44 overflow-y-auto">
        {uploads.map((item) => (
          <li key={item.id} className="px-2.5 py-1.5">
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]" title={item.path}>
                {item.path}
              </span>
              {(item.status === 'queued' || item.status === 'uploading') && (
                <TransferAction label="Cancel" onClick={() => onCancel(item)} />
              )}
              {(item.status === 'failed' || item.status === 'cancelled') && (
                <TransferAction label="Retry" onClick={() => onRetry(item)} />
              )}
            </div>
            <div className="mt-1 h-[3px] overflow-hidden bg-black/50">
              <div
                className={`h-full transition-[width] duration-150 ${barColor(item.status)}`}
                style={{ width: `${uploadPercent(item)}%` }}
              />
            </div>
            <p className={`mt-0.5 truncate text-[11px] ${textColor(item.status)}`}>
              {item.message}
            </p>
          </li>
        ))}
      </ul>
    </section>
  );
}

function TransferAction({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="shrink-0 px-1 text-[11px] text-vs-dim hover:text-vs-text"
    >
      {label}
    </button>
  );
}

function uploadPercent(item: UploadItem): number {
  if (item.status === 'success') return 100;
  if (item.sizeBytes === 0) return item.status === 'uploading' ? 50 : 0;
  return Math.max(0, Math.min(100, Math.round((item.uploadedBytes / item.sizeBytes) * 100)));
}

function barColor(status: UploadStatus): string {
  switch (status) {
    case 'success':
      return 'bg-emerald-500';
    case 'failed':
      return 'bg-red-500';
    case 'cancelled':
      return 'bg-amber-500';
    default:
      return 'bg-accent';
  }
}

function textColor(status: UploadStatus): string {
  switch (status) {
    case 'success':
      return 'text-emerald-400';
    case 'failed':
      return 'text-red-400';
    case 'cancelled':
      return 'text-amber-400';
    default:
      return 'text-vs-dim';
  }
}
