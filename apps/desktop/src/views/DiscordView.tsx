/**
 * The Discord screen.
 *
 * Everything it describes is built and tested except the connection itself, so
 * the screen says exactly that rather than showing controls that would do
 * nothing when pressed.
 */
export default function DiscordView() {
  return (
    <div className="px-8 py-7">
      <h1 className="text-xl font-semibold tracking-tight">Discord</h1>
      <p className="mt-1 text-sm text-neutral-400">
        Watch and control projects from a Discord server.
      </p>

      <section className="mt-6 rounded-xl border border-edge bg-surface p-5">
        <div className="flex items-center gap-3">
          <span className="h-2.5 w-2.5 rounded-full bg-neutral-600" aria-hidden />
          <h2 className="font-medium">Not connected</h2>
        </div>
        <p className="mt-3 max-w-2xl text-sm leading-relaxed text-neutral-400">
          The permission model, control panel, channel naming and message safety rules are built and
          tested. The gateway connection that carries them to Discord is not written yet, so no bot
          can be linked from here.
        </p>
        <button
          type="button"
          disabled
          title="The Discord connection is not implemented yet"
          className="mt-4 rounded-lg bg-white/10 px-4 py-2 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-50"
        >
          Connect a server
        </button>
      </section>

      <section className="mt-4 rounded-xl border border-edge bg-surface p-5">
        <h2 className="font-medium">What it will do</h2>
        <ul className="mt-3 space-y-2 text-sm text-neutral-400">
          <li>Give every project a log channel and a control panel channel.</li>
          <li>Start, stop, restart and inspect projects from buttons in Discord.</li>
          <li>Restrict who can do what by Discord role, with everything audited.</li>
          <li>Strip secrets and neutralise mentions before anything is posted.</li>
        </ul>
      </section>
    </div>
  );
}
