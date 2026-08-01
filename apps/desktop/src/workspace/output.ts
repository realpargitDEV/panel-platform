/**
 * The Output panel's contents.
 *
 * Every line here is written by something that actually happened in this
 * session: a file saved, an upload finished, a project start refused. It is a
 * transcript, not a log stream — the core does not stream its own log to the
 * window, and a panel that invented lines to look busy would be worse than an
 * empty one.
 */

export type OutputChannel = 'Files' | 'Transfers' | 'Project' | 'Workspace';

export type OutputLevel = 'info' | 'warn' | 'error';

export interface OutputLine {
  id: number;
  at: Date;
  channel: OutputChannel;
  level: OutputLevel;
  text: string;
}

/** How many lines are kept. Old ones fall off the top. */
export const OUTPUT_LIMIT = 500;

/** `[19:04:11]`, in local time, zero-padded. */
export function formatTime(at: Date): string {
  const pad = (value: number) => value.toString().padStart(2, '0');
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`;
}

/**
 * Append a line, dropping the oldest once the buffer is full.
 *
 * Pure, so the trimming rule is testable: a transcript that grows without
 * bound is a memory leak in a window that stays open for days.
 */
export function appendLine(lines: OutputLine[], line: OutputLine): OutputLine[] {
  const next = [...lines, line];
  return next.length > OUTPUT_LIMIT ? next.slice(next.length - OUTPUT_LIMIT) : next;
}

export function linesForChannel(lines: OutputLine[], channel: OutputChannel | 'all'): OutputLine[] {
  return channel === 'all' ? lines : lines.filter((line) => line.channel === channel);
}
