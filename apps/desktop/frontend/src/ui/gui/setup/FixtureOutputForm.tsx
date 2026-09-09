import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import type { GuiObjectRef, SetupDocument } from "../../../types";
import { ControllerUsage } from "./ControllerMembership";
import { NumberField } from "./PatchInputs";

export function FixtureOutputForm({ document }: { document: SetupDocument }) {
  const [nodeId, setNodeId] = useState<number | null>(null);
  const [reference, setReference] = useState<GuiObjectRef | null>(null);
  const [portId, setPortId] = useState<number | null>(null);
  const [channel, setChannel] = useState(1);
  const [mode, setMode] = useState<"add" | "replace">("replace");
  const [error, setError] = useState<string | null>(null);
  const pending = useAppStore((state) => state.guiEditPending);
  const controller = document.controllers.find((candidate) => candidate.sourceRef.moduleId === reference?.moduleId && candidate.sourceRef.path === reference.path && candidate.sourceRef.objectKey === reference.objectKey);
  const port = controller?.ports.find((candidate) => candidate.id === portId);
  const fixture = document.elements.find((element) => element.id === nodeId && element.kind === "fixture");
  const profile = document.fixtureProfiles.find((profile) => profile.id === fixture?.profile);
  const channels = profile === undefined ? null : Math.max(0, ...profile.definition.channels.map((channel) => channel.slot + 1));
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    if (fixture === undefined || controller === undefined || port === undefined) return;
    void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "assignFixtureOutput", node: fixture.id, controller: controller.sourceRef, port: port.id, startSlot: channel - 1, mode }))
      .then(() => { setError(null); }).catch((error: unknown) => { setError(String(error)); });
  }}>
    <h4>Assign a fixture to an output</h4>
    {error !== null && <p role="alert">{error}</p>}
    <fieldset disabled={pending || document.patchReadOnly}>
      <label>Fixture<select required value={fixture?.id ?? ""} onChange={(event) => { setNodeId(Number(event.target.value)); }}>
        <option value="" disabled>Choose a fixture</option>
        {document.elements.filter((element) => element.kind === "fixture").map((element) => <option key={element.id} value={element.id}>{element.name}</option>)}
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
      {channels !== null && <p>{profile?.name} uses {channels} channels: {channel}–{channel + channels - 1}. All fixture channels must fit on this output.</p>}
      <label>Assignment<select value={mode} onChange={(event) => { setMode(event.target.value === "add" ? "add" : "replace"); }}><option value="replace">Replace this fixture's outputs</option><option value="add">Add another copy of this output</option></select></label>
      <p>Replacement updates this fixture's guided assignments in one edit. Custom patch processing must be edited in the patch editor. Choose another free range to mirror the fixture.</p>
      {controller !== undefined && port !== undefined && <ControllerUsage controller={controller} portId={port.id} />}
      <button type="submit" disabled={fixture === undefined || controller === undefined || port === undefined}>Assign fixture output</button>
    </fieldset>
  </form>;
}
