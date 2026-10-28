import type { IoDiagnostic } from "../../types";
import { FOCUS_SIDEBAR_EVENT } from "../../commandRegistry";
import { navigateToText } from "../../workspace/navigation";

export function BlockedGui({
  reason,
  diagnostics
}: {
  reason: string;
  diagnostics: IoDiagnostic[];
}) {
  const primaryDiagnostic = diagnostics[0];
  return (
    <div className="gui-blocked">
      <strong>{reason}</strong>
      {diagnostics.length > 0 && (
        <div className="gui-diagnostics">
          {diagnostics.map((diagnostic, index) => (
            <div key={`${diagnostic.path}-${index}`}>
              {diagnostic.range ? `${diagnostic.range.start.line + 1}:${diagnostic.range.start.character + 1} ` : ""}
              {diagnostic.message}
            </div>
          ))}
        </div>
      )}
      <div className="gui-blocked-actions">
        <button
          type="button"
          onClick={() => {
            window.dispatchEvent(new CustomEvent(FOCUS_SIDEBAR_EVENT, { detail: "problems" }));
          }}
        >
          Open Problems
        </button>
        {primaryDiagnostic !== undefined && (
          <button
            type="button"
            onClick={() => void navigateToText(primaryDiagnostic.path, primaryDiagnostic.range)}
          >
            Show in Text
          </button>
        )}
      </div>
    </div>
  );
}
