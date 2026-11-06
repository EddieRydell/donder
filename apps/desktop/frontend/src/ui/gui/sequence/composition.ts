import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ReactNode } from "react";
import type { CompositionGuiEdit, GuiDocumentRequest, SequenceAutomationClip, SequenceGuiDocument, SequenceMarkCollection, SequenceParamRange } from "../../../types";
import type { AutomationClipChooser, GuiItemEdit } from "../shared";

/** What the layer and graph editors show of a sequence or console. */
export type CompositionDocument = Pick<SequenceGuiDocument, "sourceRef" | "layers" | "compositionGraph" | "curveLibrary" | "gradientLibrary"> & {
  markCollections: Pick<SequenceMarkCollection, "key">[];
};

/** A sequence's or console's layers and composition graph, with how to edit them. */
export type CompositionEditing = {
  document: CompositionDocument;
  /** How many clips play on the layer. */
  layerUses: (layerId: number) => number;
  apply: (edit: CompositionGuiEdit, origin?: GuiDocumentRequest | null) => Promise<unknown>;
  /** Rename, describe, color or delete a layer or graph node. */
  applyItem: (edit: GuiItemEdit) => Promise<unknown>;
  /** Sequences automate operator parameters with automation clips; consoles have no timeline. */
  automation: {
    automationClips: SequenceAutomationClip[];
    automationClipChooser: AutomationClipChooser;
    setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  } | null;
  /** A console links operator parameters to its knobs instead. */
  operatorParamControl: ((nodeId: number, param: string, range: SequenceParamRange | null) => ReactNode) | null;
};

export function sequenceComposition(
  document: SequenceGuiDocument,
  automation: CompositionEditing["automation"]
): CompositionEditing {
  return {
    document,
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: { type: "composition", edit } }), origin),
    applyItem: (edit) => runGuiEditCommand((request) => commands.applyGuiEdit(request, edit)),
    automation,
    operatorParamControl: null
  };
}
