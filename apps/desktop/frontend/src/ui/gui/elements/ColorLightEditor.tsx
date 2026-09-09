import type { EditElements } from "./ElementTreeEditor";
import { useState } from "react";
import { useAppStore } from "../../../store";
import type { GuiColorCapability, ElementTreeGuiDocument } from "../../../types";
import { ColorCapabilityInput } from "../setup/PatchInputs";

export function ColorLightEditor({ document, onEdit, node, initial }: { document: ElementTreeGuiDocument; onEdit: EditElements; node: number; initial: GuiColorCapability }) {
  const [capability, setCapability] = useState(initial);
  const [order, setOrder] = useState("");
  const [error, setError] = useState<string | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const components = capability.type === "discrete" ? capability.emitters.length : capability.type === "rgbw" ? 4 : 3;
  return <details className="setup-light-editor">
    <summary>Edit color capability</summary>
    <form className="setup-authoring-form" onSubmit={(event) => {
      event.preventDefault();
      const componentOrder = Array.from({ length: components }, (_, index) => index);
      if (order === "greenFirst") { componentOrder[0] = 1; componentOrder[1] = 0; }
      void onEdit( { type: "updateColorCapability", id: node, capability, componentOrder })
        .then(() => { setError(null); }).catch((error: unknown) => { setError(String(error)); });
    }}>
      {error !== null && <p role="alert">{error}</p>}
      <fieldset disabled={pending || document.readOnly}>
        <ColorCapabilityInput value={capability} onChange={(value) => { setCapability(value); setOrder(""); }} />
        <label>Output color order<select required value={order} onChange={(event) => { setOrder(event.target.value); }}>
          <option value="" disabled>Choose output order</option>
          <option value="natural">{capability.type === "discrete" ? "Declared emitter order" : capability.type === "rgbw" ? "RGBW" : "RGB"}</option>
          {capability.type !== "discrete" && <option value="greenFirst">{capability.type === "rgbw" ? "GRBW" : "GRB"}</option>}
        </select></label>
        <p>Applies this capability and order to every guided output for this light. Starting channels stay fixed; channel counts and port spans update together. Conflicts or custom routes reject the whole edit.</p>
        <button type="submit">{pending ? "Applying..." : "Apply color changes"}</button>
      </fieldset>
    </form>
  </details>;
}
