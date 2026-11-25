import { Link2 } from "lucide-react";
import { put, site } from "../edits";
import type { GuiDocument } from "../../../types";
import { Badge } from "../../kit";
import { CurveParam, GradientParam, ParamProblem } from "../sequence/params/TypedParamInput";
import { OBJECT, useFieldProblem } from "../problems";

export function LibraryEditor({ gui }: { gui: Extract<GuiDocument, { type: "curve" | "gradient" }> }) {
  const problem = useFieldProblem(OBJECT, { type: "points" });
  return <main className="object-editor">
    <header className="object-overview-header">
      <h2>{gui.document.objectKey}</h2>
      <Badge icon={Link2} tip={`Shared ${gui.type}: edits affect every parameter linked to it. Unlink a parameter to customize it alone.`} />
    </header>
    <section className="object-section"><ParamProblem problem={problem}>
      {gui.type === "curve"
        ? <CurveParam name={gui.document.objectKey} points={gui.document.points} commit={async (points) => { await put(site(null, { type: "points" }), { type: "curve", points }); }} />
        : <GradientParam name={gui.document.objectKey} space={gui.document.gradient.space} points={gui.document.gradient.stops}
          commit={async (stops) => { await put(site(null, { type: "points" }), { type: "gradient", gradient: { space: gui.document.gradient.space, stops } }); }}
          commitSpace={async (space) => { await put(site(null, { type: "points" }), { type: "gradient", gradient: { space, stops: gui.document.gradient.stops } }); }} />}
    </ParamProblem></section>
  </main>;
}
