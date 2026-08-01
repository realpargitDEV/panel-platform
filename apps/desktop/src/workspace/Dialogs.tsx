/**
 * The two things the workspace has to ask before it acts.
 *
 * Both are modal on purpose. Deleting a folder and overwriting a file are the
 * only operations here that destroy something the user cannot get back, and a
 * toast that can be missed is the wrong shape for either.
 */
import type { ReactNode } from 'react';

import type { ConflictChoice } from './conflicts';

function Modal({
  title,
  children,
  onDismiss,
}: {
  title: string;
  children: ReactNode;
  onDismiss: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-6"
      onMouseDown={onDismiss}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === 'Escape') onDismiss();
        }}
        className="w-[min(460px,92vw)] border border-vs-border bg-[#12182a] p-4 shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
      >
        {children}
      </div>
    </div>
  );
}

export function ConfirmDialog({
  title,
  detail,
  confirmLabel,
  danger,
  onConfirm,
  onCancel,
}: {
  title: string;
  detail?: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal title={title} onDismiss={onCancel}>
      <p className="text-[13px] text-vs-text">{title}</p>
      {detail && <p className="mt-1.5 text-[12px] text-vs-dim">{detail}</p>}
      <div className="mt-4 flex justify-end gap-2">
        <DialogButton onClick={onCancel}>Cancel</DialogButton>
        <DialogButton autoFocus primary danger={danger} onClick={onConfirm}>
          {confirmLabel}
        </DialogButton>
      </div>
    </Modal>
  );
}

/**
 * Asked once per drop, not once per file.
 *
 * The alternative — a prompt for every clashing name in a folder of two hundred
 * — is why people stop reading dialogs.
 */
export function ConflictDialog({
  conflicts,
  targetDirectory,
  onChoose,
  onCancel,
}: {
  conflicts: string[];
  targetDirectory: string;
  onChoose: (choice: ConflictChoice) => void;
  onCancel: () => void;
}) {
  const [first] = conflicts;

  return (
    <Modal title="Some files already exist" onDismiss={onCancel}>
      <p className="text-[13px] text-vs-text">
        {conflicts.length === 1
          ? `“${first}” already exists in ${targetDirectory || 'the project root'}.`
          : `${conflicts.length} of the files being imported already exist in ${
              targetDirectory || 'the project root'
            }.`}
      </p>

      {conflicts.length > 1 && (
        <ul className="mt-2 max-h-32 overflow-y-auto border border-vs-border bg-vs-editor p-2 font-mono text-[11px] text-vs-dim select-text">
          {conflicts.map((path) => (
            <li key={path} className="truncate">
              {path}
            </li>
          ))}
        </ul>
      )}

      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <DialogButton onClick={onCancel}>Cancel</DialogButton>
        <DialogButton onClick={() => onChoose('skip')}>Skip existing</DialogButton>
        <DialogButton autoFocus primary onClick={() => onChoose('rename')}>
          Keep both
        </DialogButton>
        <DialogButton danger onClick={() => onChoose('replace')}>
          Replace
        </DialogButton>
      </div>
      <p className="mt-2 text-right text-[11px] text-vs-dim">
        Replacing deletes the existing file first. It cannot be undone.
      </p>
    </Modal>
  );
}

function DialogButton({
  children,
  onClick,
  primary,
  danger,
  autoFocus,
}: {
  children: ReactNode;
  onClick: () => void;
  primary?: boolean;
  danger?: boolean;
  autoFocus?: boolean;
}) {
  return (
    <button
      type="button"
      autoFocus={autoFocus}
      onClick={onClick}
      className={`rounded-[2px] px-3 py-1 text-[13px] ${
        danger
          ? 'bg-red-700 text-white hover:bg-red-600'
          : primary
            ? 'bg-accent text-white hover:brightness-110'
            : 'border border-vs-border text-vs-text hover:bg-white/5'
      }`}
    >
      {children}
    </button>
  );
}
