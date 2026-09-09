import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiDocument } from "../../../types";
import { CurveParam, GradientParam } from "../sequence/params/TypedParamInput";

export function LibraryEditor({ gui }: { gui: Extract<GuiDocument, { type: "curve" | "gradient" }> }) {
  const readOnly = useAppStore((state) => state.snapshot?.activeBuffer?.readOnly ?? false);
  return <main className="setup-editor">
    <header className="object-overview-header"><div><span className="object-overview-eyebrow">{gui.document.path}</span><h2>{gui.document.objectKey}</h2></div></header>
    <section className="setup-section">
      {gui.type === "curve"
        ? <CurveParam readOnly={readOnly} name={gui.document.objectKey} points={gui.document.points} commit={async (points) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "curve", points })); }} />
        : <GradientParam readOnly={readOnly} name={gui.document.objectKey} points={gui.document.stops} commit={async (stops) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "gradient", stops })); }} />}
    </section>
  </main>;
}
