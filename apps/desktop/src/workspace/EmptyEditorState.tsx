/**
 * What the editor area shows with nothing open.
 *
 * VS Code shows its own name in outline and a short column of actions with
 * their shortcuts beside them. That is the shape here: a few things worth
 * doing, each of which runs a real command, and no paragraph explaining the
 * application to someone who has already opened it.
 */
import Icon, { type IconName } from './Icon';

export interface WelcomeAction {
  id: string;
  label: string;
  keybinding?: string;
  icon: IconName;
  enabled?: boolean;
  run: () => void;
}

export default function EmptyEditorState({
  projectName,
  actions,
}: {
  projectName: string;
  actions: WelcomeAction[];
}) {
  return (
    <div className="grid h-full place-items-center overflow-auto bg-vs-editor p-8">
      <div className="w-full max-w-md">
        <p className="mb-1 text-[26px] leading-tight font-light tracking-tight text-white/25">
          {projectName}
        </p>
        <p className="mb-6 text-[13px] text-vs-dim">No file open</p>

        <ul className="space-y-1">
          {actions.map((action) => (
            <li key={action.id}>
              <button
                type="button"
                onClick={action.run}
                disabled={action.enabled === false}
                className="flex w-full items-center gap-2 rounded-[3px] px-1.5 py-1 text-left text-[13px] text-accent hover:bg-white/5 disabled:cursor-not-allowed disabled:text-vs-dim disabled:hover:bg-transparent"
              >
                <Icon name={action.icon} size={15} />
                <span className="flex-1 truncate">{action.label}</span>
                {action.keybinding && (
                  <span className="shrink-0 text-vs-dim">{action.keybinding}</span>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
