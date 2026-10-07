import { AlertTriangle, CircleX } from "lucide-react";
import { useMemo } from "react";
import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import { THEME_METRICS } from "../theme";
import type { AppSnapshot } from "../types";
import type { AppStaticSnapshot } from "../store";
import { sameWorkspacePath } from "./helpers";
import { navigateToText } from "./navigation";

export function ProblemsView({ snapshot }: { snapshot: AppStaticSnapshot }) {
  const groups = useMemo(() => {
    const values = new Map<string, AppSnapshot["diagnostics"]>();
    for (const diagnostic of snapshot.diagnostics) {
      const entry = snapshot.projectEntries.find((candidate) =>
        sameWorkspacePath(diagnostic.path, candidate.path, snapshot.projectRoot)
      );
      const path = entry?.path ?? diagnostic.path;
      const group = values.get(path) ?? [];
      group.push(diagnostic);
      values.set(path, group);
    }
    return [...values.entries()].map(([path, diagnostics]) => [
      path,
      [...diagnostics].sort((left, right) => {
        const leftPosition = left.range?.start ?? {
          line: Number.MAX_SAFE_INTEGER,
          character: Number.MAX_SAFE_INTEGER
        };
        const rightPosition = right.range?.start ?? {
          line: Number.MAX_SAFE_INTEGER,
          character: Number.MAX_SAFE_INTEGER
        };
        return leftPosition.line - rightPosition.line
          || leftPosition.character - rightPosition.character;
      })
    ] as const);
  }, [snapshot.diagnostics, snapshot.projectEntries, snapshot.projectRoot]);
  const errors = snapshot.diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const warnings = snapshot.diagnostics.length - errors;
  return (
    <section className="sidebar-view problems-view" aria-label="Problems">
      <header className="sidebar-view-header">
        <h2>Problems</h2>
        <span>{errors} errors · {warnings} warnings</span>
      </header>
      <div className="sidebar-scroll">
        {groups.map(([path, diagnostics]) => (
          <section key={path} className="problem-file-group">
            <h3>{path}</h3>
            {diagnostics.map((diagnostic, index) => {
              const Icon = diagnostic.severity === "error" ? CircleX : AlertTriangle;
              return (
                <div className="problem-entry" key={`${diagnostic.code}:${index}`}>
                  <button
                    type="button"
                    className="problem-primary-location"
                    onClick={() => void navigateToText(path, diagnostic.range)}
                  >
                    <Icon size={THEME_METRICS.iconSizeSmall} />
                    <span>
                      {diagnostic.message}
                      {diagnostic.detail !== null && <em>{diagnostic.detail}</em>}
                      <small>
                        {diagnostic.code}
                        {diagnostic.range !== null
                          ? ` · ${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1}`
                          : ""}
                      </small>
                    </span>
                  </button>
                  {diagnostic.inclusion !== null && (
                    <button
                      type="button"
                      className="problem-fix"
                      onClick={() => {
                        const inclusion = diagnostic.inclusion;
                        if (inclusion === null) return;
                        void runSnapshotCommand(() => commands.includeDocument(snapshot.projectEpoch, inclusion));
                      }}
                    >
                      Import from the project root
                    </button>
                  )}
                  {diagnostic.related.map((related, relatedIndex) => (
                    <button
                      type="button"
                      className="problem-related-location"
                      key={`${related.path}:${relatedIndex}`}
                      onClick={() => void navigateToText(related.path, related.range)}
                    >
                      {related.message} · {related.path}
                      {related.range !== null
                        ? `:${related.range.start.line + 1}:${related.range.start.character + 1}`
                        : ""}
                    </button>
                  ))}
                </div>
              );
            })}
          </section>
        ))}
        {groups.length === 0 && <p className="empty-sidebar-state">No problems detected.</p>}
      </div>
    </section>
  );
}
