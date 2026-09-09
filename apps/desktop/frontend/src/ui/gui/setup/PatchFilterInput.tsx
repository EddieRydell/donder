import type { GuiObjectRef, PatchGuiFilter } from "../../../types";
import { ColorCapabilityInput, DimmingCurveInput, MappingInput, NumberField, ReferenceInput } from "./PatchInputs";

export const filterLabels: Record<PatchGuiFilter["type"], string> = {
  scalarToComponents: "Scalar channel values",
  colorBreakdown: "Color components", dimmingCurve: "Dimming curve", scaleInvert: "Scale / invert", fanOut: "Fan out", componentReorder: "Component order", indexedValueMapping: "Indexed mapping", quantize8: "8-bit channels", quantize16: "16-bit channels", fixtureProfileEncoding: "Fixture encoding"
};

export function newFilter(type: PatchGuiFilter["type"], profile: GuiObjectRef | undefined): PatchGuiFilter | null {
  switch (type) {
    case "scalarToComponents": return { type, width: 1 };
    case "colorBreakdown": return { type, capability: { type: "rgb" }, cellCount: 1 };
    case "dimmingCurve": return { type, curve: { type: "linear" }, width: 1 };
    case "scaleInvert": return { type, scale: 1, invert: false, width: 1 };
    case "fanOut": return { type, width: 1, outputs: 2 };
    case "componentReorder": return { type, componentsPerCell: 3, order: [0, 1, 2], cellCount: 1 };
    case "indexedValueMapping": return { type, entries: [{ id: 0, value: 0 }], width: 1 };
    case "quantize8": return { type, width: 1 };
    case "quantize16": return { type, width: 1, byteOrder: "coarseFine" };
    case "fixtureProfileEncoding": return profile === undefined ? null : { type, profile, fixtureCount: 1, slotCount: 1 };
  }
}

export function PatchFilterInput({ value, profiles, onChange }: { value: PatchGuiFilter; profiles: GuiObjectRef[]; onChange: (value: PatchGuiFilter) => void }) {
  return <div className="setup-patch-fields">
    <label>Filter<select value={value.type} onChange={(event) => {
      const type = Object.keys(filterLabels).find((type) => type === event.target.value) as PatchGuiFilter["type"] | undefined;
      if (type !== undefined) { const next = newFilter(type, profiles[0]); if (next !== null) onChange(next); }
    }}>{Object.entries(filterLabels).map(([type, label]) => <option key={type} value={type} disabled={type === "fixtureProfileEncoding" && profiles.length === 0}>{label}</option>)}</select></label>
    {"width" in value && <NumberField label="Input values" min={1} value={value.width} onChange={(width) => { onChange({ ...value, width }); }} />}
    {"cellCount" in value && <NumberField label="Cells" min={1} value={value.cellCount} onChange={(cellCount) => { onChange({ ...value, cellCount }); }} />}
    {value.type === "colorBreakdown" && <ColorCapabilityInput value={value.capability} onChange={(capability) => { onChange({ ...value, capability }); }} />}
    {value.type === "dimmingCurve" && <DimmingCurveInput value={value.curve} onChange={(curve) => { onChange({ ...value, curve }); }} />}
    {value.type === "scaleInvert" && <>
      <NumberField label="Scale" min={-Number.MAX_VALUE} step="any" value={value.scale} onChange={(scale) => { onChange({ ...value, scale }); }} />
      <label><input type="checkbox" checked={value.invert} onChange={(event) => { onChange({ ...value, invert: event.target.checked }); }} />Invert</label>
    </>}
    {value.type === "fanOut" && <NumberField label="Output ports" min={1} value={value.outputs} onChange={(outputs) => { onChange({ ...value, outputs }); }} />}
    {value.type === "componentReorder" && <>
      <NumberField label="Components per cell" min={1} value={value.componentsPerCell} onChange={(componentsPerCell) => { onChange({ ...value, componentsPerCell }); }} />
      <div className="setup-patch-fields">{value.order.map((component, index) => <div className="setup-patch-fields" key={index}>
        <NumberField label={`Output component ${index}`} value={component} onChange={(component) => { onChange({ ...value, order: value.order.map((entry, i) => i === index ? component : entry) }); }} />
        <button type="button" onClick={() => { onChange({ ...value, order: value.order.filter((_, i) => i !== index) }); }}>Remove component</button>
      </div>)}<button type="button" onClick={() => { onChange({ ...value, order: [...value.order, value.order.length] }); }}>Add component</button></div>
    </>}
    {value.type === "indexedValueMapping" && <MappingInput value={value.entries} onChange={(entries) => { onChange({ ...value, entries }); }} />}
    {value.type === "quantize16" && <label>Byte order<select value={value.byteOrder} onChange={(event) => { if (event.target.value === "coarseFine" || event.target.value === "fineCoarse") onChange({ ...value, byteOrder: event.target.value }); }}><option value="coarseFine">Coarse, fine</option><option value="fineCoarse">Fine, coarse</option></select></label>}
    {value.type === "fixtureProfileEncoding" && <>
      <ReferenceInput label="Fixture profile" value={value.profile} choices={profiles} onChange={(profile) => { onChange({ ...value, profile }); }} />
      <NumberField label="Fixture count" min={1} value={value.fixtureCount} onChange={(fixtureCount) => { onChange({ ...value, fixtureCount }); }} />
      <NumberField label="Profile channel width" min={1} value={value.slotCount} onChange={(slotCount) => { onChange({ ...value, slotCount }); }} />
    </>}
  </div>;
}
