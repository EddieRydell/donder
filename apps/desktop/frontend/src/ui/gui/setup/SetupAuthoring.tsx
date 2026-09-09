import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";
import { ControllerUsage } from "./ControllerMembership";

export function OutputAssignments({ document }: { document: SetupDocument }) {
  return <div className="setup-assignment-list">
    {document.outputAssignments.length === 0 && <p>No output assignments yet.</p>}
    {document.outputAssignments.map((assignment) => <div className="setup-summary" key={assignment.sink}>
      <span>{assignment.controller} / port {assignment.port} / channels {assignment.startChannel}–{assignment.startChannel + assignment.channelCount - 1}</span>
      <button type="button" disabled={document.patchReadOnly} onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "removeOutput", sink: assignment.sink }))}>Remove assignment</button>
    </div>)}
  </div>;
}

export function AssignOutputForm({ document }: { document: SetupDocument }) {
  const [node, setNode] = useState<number | null>(null);
  const [controllerIndex, setControllerIndex] = useState(0);
  const controller = document.controllers[controllerIndex];
  const [portId, setPortId] = useState<number | null>(null);
  const port = controller?.ports.find((candidate) => candidate.id === portId) ?? controller?.ports[0];
  const [startChannel, setStartChannel] = useState(1);
  const [greenFirst, setGreenFirst] = useState(false);
  const light = document.elements.find((element) => element.id === node);
  const components = light?.colorComponentCount ?? 0;
  const supportsGreenFirst = light?.capability?.type === "rgb" || light?.capability?.type === "rgbw";
  const [mode, setMode] = useState<"add" | "replace">("replace");
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    if (node === null || controller === undefined || port === undefined) return;
    void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, {
      type: "assignPixelOutput", node, controller: controller.sourceRef, firstPort: port.id,
      startSlot: startChannel - 1, componentOrder: Array.from({ length: components }, (_, index) => supportsGreenFirst && greenFirst && index < 2 ? 1 - index : index), mode
    }));
  }}>
    <h4>Assign a light to an output</h4>
    <label>Light<select required value={node ?? ""} onChange={(event) => { setNode(Number(event.target.value)); }}>
      <option value="" disabled>Choose a light</option>{document.elements.filter((element) => element.kind === "color").map((element) => <option key={element.id} value={element.id}>{element.name} ({element.cellCount} pixels)</option>)}
    </select></label>
    <label>Controller<select value={controllerIndex} onChange={(event) => { setControllerIndex(Number(event.target.value)); setPortId(null); }}>
      {document.controllers.map((candidate, index) => <option key={index} value={index}>{candidate.label}</option>)}
    </select></label>
    <label>First output<select value={port?.id ?? ""} onChange={(event) => { setPortId(Number(event.target.value)); }}>
      {controller?.ports.map((candidate) => <option key={candidate.id} value={candidate.id}>Port {candidate.id} ({candidate.slotCount} channels)</option>)}
    </select></label>
    <label>Start channel<input type="number" min={1} max={port?.slotCount ?? 512} value={startChannel} onChange={(event) => { setStartChannel(Number(event.target.value)); }} /></label>
    {supportsGreenFirst && <label>Color order<select value={greenFirst ? "greenFirst" : "natural"} onChange={(event) => { setGreenFirst(event.target.value === "greenFirst"); }}><option value="natural">{light.capability?.type === "rgbw" ? "RGBW" : "RGB"}</option><option value="greenFirst">{light.capability?.type === "rgbw" ? "GRBW" : "GRB"}</option></select></label>}
    {light?.capability?.type === "discrete" && <p>Channels follow the declared emitter order. Use the patch editor for a custom channel order.</p>}
    <label>Assignment<select value={mode} onChange={(event) => { setMode(event.target.value === "add" ? "add" : "replace"); }}><option value="replace">Replace this light's outputs</option><option value="add">Add another copy of this output</option></select></label>
    <p>{components > 0 ? `Each pixel uses ${components} channels. ` : ""}Longer lights continue onto the next declared output. Overlapping channels are rejected.</p>
    {controller !== undefined && port !== undefined && <ControllerUsage controller={controller} portId={port.id} />}
    <button type="submit" disabled={document.patchReadOnly || node === null || port === undefined}>Assign output</button>
  </form>;
}

