import { describe, expect, it } from 'vitest';

import { appendLine, formatTime, linesForChannel, OUTPUT_LIMIT, type OutputLine } from './output';

function line(id: number, channel: OutputLine['channel'] = 'Files'): OutputLine {
  return { id, at: new Date(0), channel, level: 'info', text: `line ${id}` };
}

describe('the output transcript', () => {
  it('appends in order', () => {
    const lines = appendLine(appendLine([], line(1)), line(2));
    expect(lines.map((entry) => entry.id)).toEqual([1, 2]);
  });

  it('drops the oldest line once it is full, so a long session cannot grow forever', () => {
    let lines: OutputLine[] = [];
    for (let id = 0; id < OUTPUT_LIMIT + 10; id += 1) lines = appendLine(lines, line(id));

    expect(lines).toHaveLength(OUTPUT_LIMIT);
    expect(lines[0]?.id).toBe(10);
    expect(lines[lines.length - 1]?.id).toBe(OUTPUT_LIMIT + 9);
  });

  it('never mutates the array it was given', () => {
    const before: OutputLine[] = [line(1)];
    appendLine(before, line(2));
    expect(before).toHaveLength(1);
  });

  it('filters to one channel, or shows everything', () => {
    const lines = [line(1, 'Files'), line(2, 'Project'), line(3, 'Files')];
    expect(linesForChannel(lines, 'Files').map((entry) => entry.id)).toEqual([1, 3]);
    expect(linesForChannel(lines, 'all')).toHaveLength(3);
  });
});

describe('timestamps', () => {
  it('pads to a fixed width so the lines stay aligned', () => {
    const at = new Date(2026, 0, 1, 9, 4, 7);
    expect(formatTime(at)).toBe('09:04:07');
  });
});
