import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { RigButtonRef, RigEffectOwner, RigGuiDocument, RigGuiEdit, RigInputDevice } from "../../../types";
import type { CompositionEditing } from "../sequence/composition";

/** Apply an edit to the open rig; a rejected edit's error is shown and rethrown. */
export async function applyRigEdit(edit: RigGuiEdit): Promise<void> {
  await runGuiEditCommand((request) => commands.applyRigGuiEdit(request, edit));
}

export function editRigEffect(owner: RigEffectOwner, edit: Extract<RigGuiEdit, { type: "editEffect" }>["edit"]) {
  return applyRigEdit({ type: "editEffect", owner, edit });
}

export function rigComposition(document: RigGuiDocument): CompositionEditing {
  return {
    document: { ...document, markCollections: document.liveMarks },
    layerUses: (layerId) => document.voices.filter((voice) => voice.effect.layerId === layerId).length
      + document.mappings.filter((mapping) => mapping.action.type === "trigger" && mapping.action.effect.layerId === layerId).length,
    apply: (edit, origin) => runGuiEditCommand((request) => commands.applyRigGuiEdit(request, { type: "composition", edit }), origin),
    automation: null
  };
}

/** The first button of the setup's first device that has one. */
export function firstButton(devices: RigInputDevice[]): RigButtonRef | null {
  for (const device of devices) {
    const button = device.buttons[0];
    if (button !== undefined) return { device: device.device, control: button.control };
  }
  return null;
}
