import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ConsoleGuiDocument } from "../../../types";
import type { ClipFieldsAdapter } from "../clip/ClipFields";
import type { CompositionEditing } from "../sequence/composition";
import { isLinked } from "./consoleEditing";
import { ParamSourceSelect } from "./ParamSourceSelect";

/** The console's layers and operator graph, whose operator parameters link to knobs. */
export function consoleComposition(document: ConsoleGuiDocument): CompositionEditing {
  return {
    document,
    layerUses: (layerId) => document.clips.filter((clip) => clip.content.type === "effect" && clip.content.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "console", edit: { type: "composition", edit } }), origin),
    automation: null,
    operatorParamControl: (nodeId, param, range) => {
      const target = { type: "compositionNodeParam", nodeId, param } as const;
      return { control: <ParamSourceSelect document={document} target={target} clip={null} label={param} range={range} />, linked: isLinked(document, target, null) };
    }
  };
}

/** A console's clips, whose parameters link to knobs and presses. */
export function consoleClipFields(document: ConsoleGuiDocument): ClipFieldsAdapter {
  return {
    document,
    paramControl: (clip, param) => {
      const target = { type: "clipParam", clipId: clip.id, param: param.name } as const;
      return {
        automationControl: param.supportsAutomation ? <ParamSourceSelect document={document} target={target} clip={clip} label={param.name} range={param.range} /> : null,
        linked: isLinked(document, target, clip)
      };
    }
  };
}
