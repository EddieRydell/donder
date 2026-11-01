import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleGuiDocument } from "../../../types";
import type { CompositionEditing } from "../sequence/composition";
import { applyConsoleItemEdit } from "./consoleEditing";
import { KnobSelect } from "./KnobSelect";

/** The console's layers and operator graph, whose operator parameters link to knobs. */
export function consoleComposition(document: ConsoleGuiDocument): CompositionEditing {
  return {
    document: { ...document, markCollections: document.liveMarks },
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit: { type: "composition", edit } }), origin),
    applyItem: applyConsoleItemEdit,
    automation: null,
    operatorParamControl: (nodeId, param) => <KnobSelect document={document} target={{ type: "compositionNodeParam", nodeId, param }} label={param} />
  };
}
