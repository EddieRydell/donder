import { Link2 } from "lucide-react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { GuiDocument } from "../../../types";
import { Badge } from "../../kit";
import { CurveParam, GradientParam } from "../sequence/params/TypedParamInput";

export function LibraryEditor({ gui }: { gui: Extract<GuiDocument, { type: "curve" | "gradient" }> }) {
  return <main className="setup-editor">
    <header className="object-overview-header">
      <div><span className="object-overview-eyebrow">{gui.document.path}</span><h2>{gui.document.objectKey}</h2></div>
      <Badge icon={Link2} tip={`Shared ${gui.type}: edits affect every parameter linked to it. Unlink a parameter to customize it alone.`} />
    </header>
    <section className="setup-section">
      {gui.type === "curve"
        ? <CurveParam name={gui.document.objectKey} points={gui.document.points} commit={async (points) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "curve", points })); }} />
        : <GradientParam name={gui.document.objectKey} space={gui.document.gradient.space} points={gui.document.gradient.stops}
          commit={async (stops) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "gradient", gradient: { space: gui.document.gradient.space, stops } })); }}
          commitSpace={async (space) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "gradient", gradient: { space, stops: gui.document.gradient.stops } })); }} />}
    </section>
  </main>;
}
