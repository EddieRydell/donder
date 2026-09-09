import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiObjectRef, SetupDocument, SetupControlOutputMapping } from "../../../types";
import { ControllerUsage } from "./ControllerMembership";
import { NumberField } from "./PatchInputs";

export function ControlOutputForm({ document }: { document: SetupDocument }) {
  const [nodeId, setNodeId] = useState<number | null>(null);
  const [reference, setReference] = useState<GuiObjectRef | null>(null);
  const [portId, setPortId] = useState<number | null>(null);
  const [channel, setChannel] = useState(1);
  const [values, setValues] = useState<Record<number, string>>({});
  const [mode, setMode] = useState<"add" | "replace">("replace");
  const [error, setError] = useState<string | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const controller = document.controllers.find((candidate) => candidate.sourceRef.moduleId === reference?.moduleId && candidate.sourceRef.path === reference.path && candidate.sourceRef.objectKey === reference.objectKey);
  const port = controller?.ports.find((candidate) => candidate.id === portId);
  const control = document.elements.find((element) => element.id === nodeId && (element.kind === "scalar" || element.kind === "indexed"));
  const definition = control?.controlDefinition;
  const channels = definition?.type === "scalar" || definition?.type === "indexed" ? definition.cells : null;
  const options = definition?.type === "indexed" ? definition.options : [];
  const mappedOptions = options.some((option) => option.id === 0) ? options : [{ id: 0, name: "Inactive (no active clip)" }, ...options];
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    if (control === undefined || controller === undefined || port === undefined || definition === undefined || definition === null || definition.type === "fixture") return;
    const mapping: SetupControlOutputMapping = definition.type === "scalar" ? { type: "scalar" } : {
      type: "indexed", entries: mappedOptions.map((option) => ({ id: option.id, value: Number(values[option.id]) })),
    };
    void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "assignControlOutput", assignment: { node: control.id, controller: controller.sourceRef, port: port.id, startSlot: channel - 1, mapping }, mode }))
      .then(() => { setError(null); }).catch((error: unknown) => { setError(String(error)); });
  }}>
    <h4>Assign a dimmer or indexed control to an output</h4>
    {error !== null && <p role="alert">{error}</p>}
    <fieldset disabled={pending || document.patchReadOnly}>
      <label>Control<select required value={control?.id ?? ""} onChange={(event) => { setNodeId(Number(event.target.value)); setValues({}); }}>
        <option value="" disabled>Choose a control</option>
        {document.elements.filter((element) => (element.kind === "scalar" || element.kind === "indexed")).map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
      </select></label>
      <label>Controller<select required value={controller === undefined ? "" : document.controllers.indexOf(controller)} onChange={(event) => { const selected = document.controllers[Number(event.target.value)]; if (selected !== undefined) { setReference(selected.sourceRef); setPortId(null); } }}>
        <option value="" disabled>Choose a controller</option>
        {document.controllers.map((controller, index) => <option key={index} value={index}>{controller.label}</option>)}
      </select></label>
      <label>Output<select required value={port?.id ?? ""} onChange={(event) => { setPortId(Number(event.target.value)); }}>
        <option value="" disabled>Choose an output</option>
        {controller?.ports.map((port) => <option key={port.id} value={port.id}>Port {port.id} ({port.slotCount} channels)</option>)}
      </select></label>
      <NumberField label="First channel" min={1} {...(port === undefined ? {} : { max: port.slotCount })} value={channel} onChange={setChannel} />
      {channels !== null && <p>{control?.name} uses {channels} channels: {channel}–{channel + channels - 1}. All control channels must fit on this output.</p>}
      {definition?.type === "indexed" && <div className="setup-patch-list">
        <p>Enter the channel value for each option from the device's channel chart. ID 0 is used when no control clip is active. The same mapping applies to every cell.</p>
        {mappedOptions.map((option) => <label key={option.id}>{option.name} channel value<input type="number" required min={0} max={255} step={1} value={values[option.id] ?? ""} onChange={(event) => { setValues({ ...values, [option.id]: event.target.value }); }} /></label>)}
      </div>}
      {definition?.type === "scalar" && <p>Each cell uses one 8-bit channel: level 0 is channel value 0; level 1 is 255.</p>}
      <label>Assignment<select value={mode} onChange={(event) => { setMode(event.target.value === "add" ? "add" : "replace"); }}><option value="replace">Replace this control's outputs</option><option value="add">Add another copy of this output</option></select></label>
      <p>Replacement updates this control's guided assignments in one edit. Custom patch processing must be edited in the patch editor. Choose another free range to mirror the control.</p>
      {controller !== undefined && port !== undefined && <ControllerUsage controller={controller} portId={port.id} />}
      <button type="submit" disabled={control === undefined || controller === undefined || port === undefined}>Assign control output</button>
    </fieldset>
  </form>;
}
