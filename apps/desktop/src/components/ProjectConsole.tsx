/**
 * One project's output.
 *
 * Polls with a cursor rather than re-reading, so a project producing a line a
 * second does not re-send its whole buffer every tick. A stopped project has no
 * cursor — its console comes from the file on disk — so this re-reads that
 * instead, and says which of the two the reader is looking at.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

import { errorMessage, projectConsole, type ConsoleLine } from '../api';
import { Badge, Button, Card, CardHeader } from '../ui/primitives';

/** How often a running project's console is polled. */
const POLL_MS = 1000;

/**
 * How many lines are kept on screen.
 *
 * The buffer behind it keeps two thousand; rendering all of them as DOM nodes
 * is what makes a console the slowest thing in a window, and nobody scrolls
 * back two thousand lines in a panel this size.
 */
const MAX_LINES = 500;

const STREAM_CLASS: Record<ConsoleLine['stream'], string> = {
  stdout: 'text-ink',
  stderr: 'text-danger',
  system: 'text-muted italic',
};

export default function ProjectConsole({
  projectId,
  fill = false,
}: {
  projectId: string;
  /**
   * Fill the pane instead of sitting in a card.
   *
   * The console appears in two places: as one section of the project detail
   * page, where a card is the right container, and as a whole workspace pane,
   * where a card would be a rounded box inside a rounded box with its own
   * scrollbar inside the pane's.
   */
  fill?: boolean;
}) {
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [live, setLive] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [following, setFollowing] = useState(true);

  const cursor = useRef(0);
  const scroller = useRef<HTMLDivElement | null>(null);

  // Reset when the project changes, or the next poll would append one
  // project's output to another's.
  useEffect(() => {
    cursor.current = 0;
    setLines([]);
    setFailure(null);
  }, [projectId]);

  const poll = useCallback(() => {
    projectConsole(projectId, cursor.current)
      .then((page) => {
        setLive(page.live);
        setFailure(null);

        if (page.live) {
          cursor.current = page.cursor;
          if (page.lines.length > 0) {
            setLines((previous) => [...previous, ...page.lines].slice(-MAX_LINES));
          }
          return;
        }

        // A file has no cursor: what comes back is the whole tail, so it
        // replaces rather than appends.
        cursor.current = 0;
        setLines(page.lines.slice(-MAX_LINES));
      })
      .catch((error: unknown) => setFailure(errorMessage(error)));
  }, [projectId]);

  useEffect(() => {
    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => clearInterval(timer);
  }, [poll]);

  // Before paint, so following does not show a frame of the old position.
  useLayoutEffect(() => {
    if (!following) return;
    const element = scroller.current;
    if (element !== null) element.scrollTop = element.scrollHeight;
  }, [lines, following]);

  /**
   * Following stops when the reader scrolls up and resumes at the bottom.
   *
   * Rather than a button they have to remember to press: someone scrolling back
   * to read something is telling us not to move the view, and someone
   * scrolling back down is telling us they are done.
   */
  function onScroll() {
    const element = scroller.current;
    if (element === null) return;
    const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
    setFollowing(atBottom);
  }

  const body = (
    <>
      <div
        className={
          fill
            ? 'flex shrink-0 items-center justify-between gap-2 border-b border-edge px-2.5'
            : 'hidden'
        }
        style={fill ? { height: 'var(--h-panel-header)' } : undefined}
      >
        <span className="text-[11px] font-semibold tracking-wide text-muted uppercase">
          Console
        </span>
        <div className="flex items-center gap-1.5">
          <Badge tone={live ? 'ok' : 'neutral'} dot>
            {live ? 'Live' : 'Not running'}
          </Badge>
          {!following && (
            <button
              type="button"
              onClick={() => setFollowing(true)}
              className="h-[22px] rounded-[3px] border border-edge px-1.5 text-[11px] text-muted hover:bg-raised hover:text-ink"
            >
              Follow
            </button>
          )}
        </div>
      </div>

      {failure !== null && <p className="px-3 py-1.5 text-[12px] text-danger">{failure}</p>}

      <div
        ref={scroller}
        onScroll={onScroll}
        className={`overflow-y-auto bg-canvas px-3 py-2 font-mono text-[12.5px] leading-[1.5] ${
          fill ? 'min-h-0 flex-1' : 'max-h-[420px] min-h-[180px] border-t border-edge bg-sunken'
        }`}
      >
        {lines.length === 0 ? (
          <p className="py-6 text-center text-[12px] text-muted">
            {live
              ? 'Nothing has been written yet.'
              : 'This project has not produced any output yet.'}
          </p>
        ) : (
          lines.map((line) => (
            <div
              key={`${line.seq}-${line.at}`}
              className="flex gap-2.5 whitespace-pre-wrap break-all"
            >
              <span className="shrink-0 select-none tabular text-faint">
                {line.at.slice(11, 19)}
              </span>
              <span className={STREAM_CLASS[line.stream]}>{line.text}</span>
            </div>
          ))
        )}
      </div>
    </>
  );

  if (fill) {
    return <div className="flex h-full min-h-0 flex-col">{body}</div>;
  }

  return (
    <Card>
      <CardHeader
        title="Console"
        actions={
          <div className="flex items-center gap-2">
            <Badge tone={live ? 'ok' : 'neutral'} dot>
              {live ? 'Live' : 'Not running'}
            </Badge>
            {!following && (
              <Button
                size="sm"
                onClick={() => {
                  setFollowing(true);
                }}
              >
                Follow
              </Button>
            )}
          </div>
        }
      />

      {failure !== null && <p className="px-4 pb-2 text-[12px] text-danger">{failure}</p>}

      <div
        ref={scroller}
        onScroll={onScroll}
        className="max-h-[420px] min-h-[180px] overflow-y-auto border-t border-edge bg-sunken px-3 py-2 font-mono text-[12px] leading-[1.55]"
      >
        {lines.length === 0 ? (
          <p className="py-6 text-center text-[12px] text-muted">
            {live
              ? 'Nothing has been written yet.'
              : 'This project has not produced any output yet.'}
          </p>
        ) : (
          lines.map((line) => (
            <div
              key={`${line.seq}-${line.at}`}
              className="flex gap-2.5 whitespace-pre-wrap break-all"
            >
              <span className="shrink-0 select-none tabular text-muted/70">
                {line.at.slice(11, 19)}
              </span>
              <span className={STREAM_CLASS[line.stream]}>{line.text}</span>
            </div>
          ))
        )}
      </div>
    </Card>
  );
}
