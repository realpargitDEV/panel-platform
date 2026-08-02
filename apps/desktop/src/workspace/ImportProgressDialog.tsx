/**
 * One organised import, while it runs.
 *
 * Deliberately does not close itself on failure: an operation that ended with
 * files half-imported and a rollback that could not finish is the one moment
 * the user most needs to read what happened, and a dialog that vanishes takes
 * the paths with it.
 */
import Icon from './Icon';
import { formatBytes } from '../lib/format';
import {
  describePhase,
  hasIncompleteRollback,
  percentComplete,
  type ImportOperationProgress,
} from './importOperation';

export default function ImportProgressDialog({
  operation,
  onCancel,
  onClose,
}: {
  operation: ImportOperationProgress;
  onCancel: () => void;
  onClose: () => void;
}) {
  const percent = percentComplete(operation);
  const finished = operation.phase === 'completed' || operation.phase === 'failed';
  const failed = operation.phase === 'failed';
  const rollingBack = operation.phase === 'rolling-back';

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-6">
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Import progress"
        className="w-[min(560px,94vw)] border border-vs-border bg-[#12182a] shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
      >
        <div className="flex items-start gap-2.5 border-b border-vs-border px-4 py-3">
          <span
            className={`mt-0.5 ${
              failed ? 'text-red-400' : finished ? 'text-emerald-400' : 'text-accent'
            }`}
          >
            <Icon name={failed ? 'warning' : finished ? 'check' : 'refresh'} size={16} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="text-[14px] font-semibold text-vs-text">
              {finished ? 'Import finished' : 'Importing'}
            </h2>
            <p aria-live="polite" className="mt-0.5 text-[12px] text-vs-dim">
              {describePhase(operation)}
            </p>
          </div>
        </div>

        <div className="px-4 py-3">
          <div
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent === null ? undefined : Math.round(percent)}
            aria-valuetext={percent === null ? 'Working, size unknown' : undefined}
            className="h-1.5 overflow-hidden rounded-full bg-black/50"
          >
            <div
              className={`h-full transition-[width] duration-150 ${
                failed ? 'bg-red-500' : rollingBack ? 'bg-amber-500' : 'bg-accent'
              } ${percent === null ? 'w-1/3 animate-pulse' : ''}`}
              style={percent === null ? undefined : { width: `${percent}%` }}
            />
          </div>

          <div className="mt-2 flex flex-wrap items-baseline justify-between gap-x-4 text-[12px] text-vs-dim">
            <span>
              {operation.totalGroups > 1 && (
                <>
                  Group {Math.min(operation.currentGroupIndex + 1, operation.totalGroups)} of{' '}
                  {operation.totalGroups}
                  {operation.currentGroupName ? ` · ${operation.currentGroupName}` : ''}
                  {' · '}
                </>
              )}
              {operation.processedEntries} of {operation.totalEntries} items
            </span>
            <span>
              {operation.totalBytes === null
                ? 'size unknown'
                : `${formatBytes(operation.processedBytes)} of ${formatBytes(operation.totalBytes)}`}
            </span>
          </div>

          {operation.currentSourcePath && !finished && (
            <p className="mt-1 truncate font-mono text-[11px] text-vs-dim">
              {operation.currentSourcePath}
            </p>
          )}

          {rollingBack && (
            <p className="mt-2 text-[12px] text-amber-300">
              Undoing {operation.rollbackProcessed} of {operation.rollbackTotal} changes. Files that
              were already in the project are not touched.
            </p>
          )}

          {operation.errors.length > 0 && (
            <div className="mt-3 max-h-32 overflow-y-auto border border-red-900/60 bg-red-950/30 p-2">
              <p className="mb-1 text-[11px] font-medium text-red-300">
                {hasIncompleteRollback(operation)
                  ? 'Some changes could not be undone:'
                  : `${operation.errors.length} problem${operation.errors.length === 1 ? '' : 's'}:`}
              </p>
              <ul className="space-y-0.5">
                {operation.errors.map((error, index) => (
                  <li
                    key={`${error.path}-${index}`}
                    className="text-[11px] break-words text-vs-dim"
                  >
                    <span className="font-mono text-vs-text">{error.path}</span> — {error.message}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {operation.phase === 'completed' && operation.errors.length === 0 && (
            <p className="mt-2 text-[12px] text-emerald-400">
              {operation.cancellationRequested
                ? 'Cancelled. Nothing was left half-imported.'
                : `${operation.totalEntries} item${operation.totalEntries === 1 ? '' : 's'} imported.`}
            </p>
          )}
        </div>

        <div className="flex items-center gap-2 border-t border-vs-border px-4 py-2.5">
          <span className="flex-1 text-[11px] text-vs-dim">
            {!finished && !operation.canCancel && 'This step cannot be interrupted.'}
          </span>
          {!finished && (
            <button
              type="button"
              disabled={!operation.canCancel || operation.cancellationRequested}
              onClick={onCancel}
              className="rounded-[2px] border border-vs-border px-3 py-1 text-[13px] text-vs-text hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {operation.cancellationRequested ? 'Cancelling…' : 'Cancel'}
            </button>
          )}
          {finished && (
            <button
              type="button"
              autoFocus
              onClick={onClose}
              className="rounded-[2px] bg-accent px-3 py-1 text-[13px] font-medium text-white hover:brightness-110"
            >
              Close
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
