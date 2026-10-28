import type { SequenceEditorHost } from "../../../editor/host";
import type { CompositionGuiEdit, GuiDocumentRequest, SequenceAutomationClip, SequenceGuiDocument, SequenceMarkCollection } from "../../../editor/types";
import type { AutomationClipChooser, GuiItemEdit } from "../shared";

/** What the layer and graph editors show of a sequence or rig. */
export type CompositionDocument = Pick<SequenceGuiDocument, "sourceRef" | "layers" | "compositionGraph" | "curveLibrary" | "gradientLibrary"> & {
  markCollections: Pick<SequenceMarkCollection, "key">[];
};

/** A sequence's or rig's layers and composition graph, with how to edit them. */
export type CompositionEditing = {
  document: CompositionDocument;
  /** How many clips play on the layer. */
  layerUses: (layerId: number) => number;
  apply: (edit: CompositionGuiEdit, origin?: GuiDocumentRequest | null) => Promise<unknown>;
  /** Rename, describe, color or delete a layer or graph node. */
  applyItem: (edit: GuiItemEdit) => Promise<unknown>;
  /** Sequences automate operator parameters; rigs have no timeline to automate on. */
  automation: {
    automationClips: SequenceAutomationClip[];
    automationClipChooser: AutomationClipChooser;
    setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
  } | null;
};

export function sequenceComposition(
  host: SequenceEditorHost,
  document: SequenceGuiDocument,
  automation: CompositionEditing["automation"]
): CompositionEditing {
  return {
    document,
    layerUses: (layerId) => document.clips.filter((clip) => clip.layerId === layerId).length,
    apply: (edit, origin) => host.runGuiEditCommand((request) => host.commands.applyGuiEdit(request, { type: "sequence", edit: { type: "composition", edit } }), origin),
    applyItem: (edit) => host.runGuiEditCommand((request) => host.commands.applyGuiEdit(request, edit)),
    automation
  };
}
