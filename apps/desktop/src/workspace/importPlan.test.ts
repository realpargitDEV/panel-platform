import { describe, expect, it } from 'vitest';

import type { ImportCandidate } from '../api';
import { describePlan, explainDetection, planImport } from './importPlan';

function project(name: string, signals = ['package.json', 'src/']): ImportCandidate {
  return {
    path: `C:\\drop\\${name}`,
    name,
    isDirectory: true,
    isProject: true,
    score: 4,
    signals,
    children: ['package.json', 'src/'],
    childCount: 2,
    ecosystem: 'Node.js',
    isMonorepo: false,
    nested: [],
  };
}

function folder(name: string): ImportCandidate {
  return {
    path: `C:\\drop\\${name}`,
    name,
    isDirectory: true,
    isProject: false,
    score: 0,
    signals: [],
    children: [],
    childCount: 0,
    ecosystem: null,
    isMonorepo: false,
    nested: [],
  };
}

function file(name: string): ImportCandidate {
  return {
    path: `C:\\drop\\${name}`,
    name,
    isDirectory: false,
    isProject: false,
    score: 0,
    signals: [],
    children: [],
    childCount: 0,
    ecosystem: null,
    isMonorepo: false,
    nested: [],
  };
}

describe('deciding what to unwrap', () => {
  it('unwraps a single dropped project, so its files land at the destination', () => {
    // The bug: keeping the folder produced RomiPlayoff/package.json inside a
    // project that was already RomiPlayoff.
    const plan = planImport([project('RomiPlayoff')]);
    expect(plan.unwrapPaths).toEqual(['C:\\drop\\RomiPlayoff']);
    expect(plan.unwraps).toBe(true);
  });

  it('keeps an ordinary folder as a folder', () => {
    const plan = planImport([folder('holiday photos')]);
    expect(plan.unwrapPaths).toEqual([]);
    expect(plan.unwraps).toBe(false);
  });

  it('never unwraps a file', () => {
    expect(planImport([file('notes.txt')]).unwrapPaths).toEqual([]);
  });

  it('keeps several projects apart rather than merging them into one root', () => {
    // There is no answer to which of two projects "wins" the root, and mixing
    // their files is unrecoverable.
    const plan = planImport([project('bot'), project('dashboard')]);
    expect(plan.unwrapPaths).toEqual([]);
    expect(plan.projects).toHaveLength(2);
  });

  it('unwraps the one project even when loose files came with it', () => {
    const plan = planImport([project('bot'), file('notes.txt'), folder('assets')]);
    expect(plan.unwrapPaths).toEqual(['C:\\drop\\bot']);
    expect(plan.files.map((item) => item.name)).toEqual(['notes.txt']);
    expect(plan.folders.map((item) => item.name)).toEqual(['assets']);
  });

  it('imports everything that was dropped, whatever it decided about wrapping', () => {
    const plan = planImport([project('bot'), file('notes.txt')]);
    expect(plan.sourcePaths).toEqual(['C:\\drop\\bot', 'C:\\drop\\notes.txt']);
  });

  it('handles an empty drop without inventing anything', () => {
    const plan = planImport([]);
    expect(plan.sourcePaths).toEqual([]);
    expect(plan.unwraps).toBe(false);
  });
});

describe('describing the plan', () => {
  it('says the contents are what is being imported when unwrapping', () => {
    expect(describePlan(planImport([project('bot')]), '')).toBe(
      'Importing the contents of bot into the project root.',
    );
  });

  it('names the destination folder when there is one', () => {
    expect(describePlan(planImport([file('a.txt')]), 'src')).toBe('Importing 1 file into src.');
  });

  it('counts each kind separately', () => {
    const plan = planImport([project('a'), project('b'), folder('c'), file('d')]);
    expect(describePlan(plan, '')).toBe(
      'Importing 2 projects, 1 folder and 1 file into the project root.',
    );
  });
});

describe('explaining a decision', () => {
  it('lists the markers that made it a project', () => {
    expect(explainDetection(project('bot'))).toBe('Detected as a project: package.json, src/.');
  });

  it('says when markers were found but were not enough', () => {
    const weak: ImportCandidate = { ...folder('thing'), signals: ['.gitignore'], score: 1 };
    expect(explainDetection(weak)).toContain('not enough');
  });

  it('says plainly when nothing was found', () => {
    expect(explainDetection(folder('photos'))).toContain('ordinary folder');
  });

  it('truncates a long list rather than printing forty names', () => {
    const many = project('big', ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']);
    expect(explainDetection(many)).toContain('and 2 more');
  });
});
