/**
 * The Problems list.
 *
 * These are Monaco's own markers, and nothing else. The editor bundles language
 * services for TypeScript, JavaScript, JSON, CSS and HTML, and those services
 * genuinely analyse the open file — so a syntax error in a `.ts` file is a real
 * finding, reported by the thing that found it.
 *
 * A file in a language with no bundled service produces no markers, and the
 * panel says so rather than claiming the file is clean. Inventing a linter for
 * the other languages would mean running one on the host, which this product
 * deliberately does not do.
 */
import * as monaco from 'monaco-editor';
import { useEffect, useState } from 'react';

export interface Problem {
  /** The project-relative path, recovered from the model's URI. */
  path: string;
  severity: 'error' | 'warning' | 'info';
  message: string;
  line: number;
  column: number;
  /** Which service said so, e.g. "typescript". */
  source: string;
}

export interface ProblemCounts {
  errors: number;
  warnings: number;
}

/** `project:/src/index.ts` is the model for `src/index.ts`. */
export function pathFromModelUri(uri: monaco.Uri): string {
  return uri.path.replace(/^\/+/, '');
}

function severityOf(marker: monaco.editor.IMarker): Problem['severity'] | null {
  switch (marker.severity) {
    case monaco.MarkerSeverity.Error:
      return 'error';
    case monaco.MarkerSeverity.Warning:
      return 'warning';
    case monaco.MarkerSeverity.Info:
      return 'info';
    default:
      // Hints are editor suggestions rather than findings, and listing them
      // would bury the two severities anyone acts on.
      return null;
  }
}

/**
 * Every marker across the open files, refreshed as the language services
 * report.
 *
 * Markers are also read once on mount: a file opened before this hook was
 * mounted has already been analysed, and waiting for the next change event
 * would show an empty panel over a file full of errors.
 */
export function useProblems(): Problem[] {
  const [problems, setProblems] = useState<Problem[]>([]);

  useEffect(() => {
    function collect() {
      const collected: Problem[] = [];
      for (const marker of monaco.editor.getModelMarkers({})) {
        const severity = severityOf(marker);
        if (!severity) continue;
        collected.push({
          path: pathFromModelUri(marker.resource),
          severity,
          message: marker.message,
          line: marker.startLineNumber,
          column: marker.startColumn,
          source: marker.source ?? marker.owner,
        });
      }
      collected.sort(bySeverityThenPosition);
      setProblems(collected);
    }

    collect();
    const subscription = monaco.editor.onDidChangeMarkers(collect);
    return () => subscription.dispose();
  }, []);

  return problems;
}

export function countProblems(problems: Problem[]): ProblemCounts {
  return {
    errors: problems.filter((problem) => problem.severity === 'error').length,
    warnings: problems.filter((problem) => problem.severity === 'warning').length,
  };
}

const ORDER: Record<Problem['severity'], number> = { error: 0, warning: 1, info: 2 };

function bySeverityThenPosition(left: Problem, right: Problem): number {
  return (
    ORDER[left.severity] - ORDER[right.severity] ||
    left.path.localeCompare(right.path) ||
    left.line - right.line ||
    left.column - right.column
  );
}
