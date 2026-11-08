import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleGuiDocument } from "../../../types";
import type { CompositionEditing } from "../sequence/composition";
import { applyConsoleItemEdit, isLinked } from "./consoleEditing";
import { ParamSourceSelect } from "./ParamSourceSelect";

/** The console's layers and operator graph, whose operator parameters link to knobs. */
export function consoleComposition(document: ConsoleGuiDocument): CompositionEditing {
  return {
    document: { ...document, markCollections: document.liveMarks },
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit: { type: "composition", edit } }), origin),
    applyItem: applyConsoleItemEdit,
    automation: null,
    operatorParamControl: (nodeId, param, range) => {
      const target = { type: "compositionNodeParam", nodeId, param } as const;
      return { control: <ParamSourceSelect document={document} target={target} clip={null} label={param} range={range} />, linked: isLinked(document, target, null) };
    }
  };
}
