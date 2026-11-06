import type { IoDiagnostic } from "../../types";
import { focusSidebar } from "../uiEvents";
import { navigateToText } from "../../workspace/navigation";
import { Button } from "../kit";

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
        <Button onClick={() => { focusSidebar("problems"); }}>
          Problems
        </Button>
        {primaryDiagnostic !== undefined && (
          <Button onClick={() => void navigateToText(primaryDiagnostic.path, primaryDiagnostic.range)}>
            Show in text
          </Button>
        )}
      </div>
    </div>
  );
}
