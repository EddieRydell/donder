import { CaseSensitive } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../api";
import { THEME_METRICS } from "../theme";
import type { ProjectSearchResponse } from "../types";
import type { AppStaticSnapshot } from "../store";
import { locationRange } from "./helpers";
import { navigateToText } from "./navigation";
import { IconButton, TextField, Tip } from "../ui/kit";

export function SearchView({ snapshot }: { snapshot: AppStaticSnapshot }) {
  const [query, setQuery] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const [response, setResponse] = useState<ProjectSearchResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  useEffect(() => {
    if (query.trim() === "") {
      requestId.current += 1;
      return;
    }
    const current = ++requestId.current;
    const timer = window.setTimeout(() => {
      void commands.searchProject({ requestId: current, query, matchCase })
        .then((next) => {
          if (next.requestId === requestId.current) {
            setResponse(next);
            setError(null);
          }
        })
        .catch((reason: unknown) => {
          if (current === requestId.current) setError(String(reason));
        });
    }, THEME_METRICS.searchDebounceMs);
    return () => { window.clearTimeout(timer); };
  }, [matchCase, query, snapshot.projectRevision]);

  const grouped = useMemo(() => {
    const groups = new Map<string, NonNullable<typeof response>["matches"]>();
    for (const match of response?.matches ?? []) {
      const values = groups.get(match.path) ?? [];
      values.push(match);
      groups.set(match.path, values);
    }
    return [...groups.entries()];
  }, [response]);

  return (
    <section className="sidebar-view search-view" aria-label="Search">
      <header className="sidebar-view-header"><h2>Search</h2></header>
      <div className="search-controls">
        <TextField name="Search project files" placeholder="Search" value={query} onCommit={setQuery} live autoFocus />
        <IconButton icon={CaseSensitive} label="Match case" pressed={matchCase} onClick={() => { setMatchCase((value) => !value); }} />
      </div>
      <div className="sidebar-scroll">
        {query.trim() !== "" && error !== null && <p className="sidebar-error">{error}</p>}
        {query.trim() !== "" && response !== null && (
          <Tip label={[
            response.truncated ? "First 500 shown" : null,
            `Skipped ${response.skippedBinary} binary and ${response.skippedOversized} oversized files`
          ].filter((part) => part !== null).join(" · ")}>
            <p className="search-summary">{response.matches.length}{response.truncated ? "+" : ""} matches</p>
          </Tip>
        )}
        {grouped.map(([path, matches]) => (
          <section key={path} className="search-file-group">
            <h3>{path}</h3>
            {matches.map((match, index) => (
              <button
                type="button"
                key={`${match.kind}:${match.line}:${match.column}:${index}`}
                onClick={() => void navigateToText(
                  match.path,
                  match.kind === "content"
                    ? locationRange(match.line, match.column, query.length)
                    : null
                )}
              >
                <span className="search-location">
                  {match.kind === "filename" ? "File name" : `${match.line + 1}:${match.column + 1}`}
                </span>
                <span>{match.preview}</span>
              </button>
            ))}
          </section>
        ))}
      </div>
    </section>
  );
}
