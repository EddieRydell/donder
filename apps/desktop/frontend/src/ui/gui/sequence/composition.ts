import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { ReactNode } from "react";
import type { CompositionGuiEdit, GuiDocumentRequest, GuiEditUpdate, GuiClip, SequenceGuiDocument, SequenceMarkCollection, SequenceParamRange } from "../../../types";

/** What the layer and graph editors show of a sequence or console. */
export type CompositionDocument = Pick<SequenceGuiDocument, "sourceRef" | "layers" | "compositionGraph" | "curveLibrary" | "gradientLibrary"> & {
  markCollections: Pick<SequenceMarkCollection, "id" | "name">[];
};

/** A sequence's or console's layers and composition graph, with how to edit them. */
export type CompositionEditing = {
  document: CompositionDocument;
  /** How many clips play on the layer. */
  layerUses: (layerId: number) => number;
  /** Resolves to null when the edit was not applied. */
  apply: (edit: CompositionGuiEdit, origin?: GuiDocumentRequest | null) => Promise<GuiEditUpdate | null>;
  /** Sequences bind operator parameters to their envelopes; consoles link them to controls. */
  automation: { envelopes: GuiClip[] } | null;
  /** A console links operator parameters to its knobs instead, and says which are linked. */
  operatorParamControl: ((nodeId: number, param: string, range: SequenceParamRange | null) => { control: ReactNode; linked: boolean }) | null;
};

export function sequenceComposition(
  document: SequenceGuiDocument,
  automation: CompositionEditing["automation"]
): CompositionEditing {
  return {
    document,
    layerUses: (layerId) => document.clips.filter((clip) => clip.content.type === "effect" && clip.content.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "sequence", edit: { type: "composition", edit } }), origin),
    automation,
    operatorParamControl: null
  };
}
