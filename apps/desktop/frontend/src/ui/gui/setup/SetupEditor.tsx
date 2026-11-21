import { guiObjectKey } from "../../../workspace/guiIdentity";
import { ControllerForm } from "../controller/ControllerForm";
import { Cable, CircuitBoard, Keyboard, LayoutGrid, Plus } from "lucide-react";
import { InputDeviceForm } from "../inputDevice/InputDeviceForm";
import { AvailableInputDevices } from "./InputDeviceMembership";
import { useCallback, useState, type ReactNode } from "react";

import type { GuiEditCommand, GuiInputControl, GuiInputDriver, SetupControllerConfig, SetupControllerPort, SetupGuiDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { AddRow, Dialog, Field, IconButton, TextField } from "../../kit";
import { AvailableControllers } from "./ControllerMembership";
import { MemberActions, memberName } from "../MemberActions";
import { countOf } from "../../count";

export function SetupEditor({ document }: { document: SetupGuiDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div><span className="object-overview-eyebrow">Display setup</span><h2>{document.sourceRef.ownedPath.length > 0 ? "Display setup" : document.sourceRef.objectKey}</h2></div>
        <span>{document.controllers.length} controllers · {document.inputDevices.length} input devices</span>
      </header>
      <section className="object-overview-group">
        <h3>Composition</h3>
        <div className="object-overview-row-with-actions"><SetupRow icon={<LayoutGrid aria-hidden="true" />} title="Layout" onOpen={() => void navigateToGuiObject(document.layoutRef)} /><MemberActions source={document.layoutRef} label={memberName(document.layoutRef)} deletable={false} /></div>
        <div className="object-overview-row-with-actions"><SetupRow icon={<Cable aria-hidden="true" />} title="Patch" onOpen={() => void navigateToGuiObject(document.patchRef)} /><MemberActions source={document.patchRef} label={memberName(document.patchRef)} deletable={false} /></div>
      </section>
      <CreationGroup title="Controllers" addLabel="Add controller" empty={document.controllers.length === 0} form={(close) => <>
        <AddController newName={document.newControllerName} onApplied={close} />
        <AvailableControllers document={document} />
      </>}>
        {document.controllers.map((controller) => <div className="object-overview-row-with-actions" key={guiObjectKey(controller.sourceRef)}>
          <SetupRow icon={<CircuitBoard aria-hidden="true" />} title={controller.label} detail={controller.config.type === "donder" ? countOf(controller.ports.length, "output") : countOf(controller.config.universes.count, "universe")} onOpen={() => void navigateToGuiObject(controller.sourceRef)} />
          <MemberActions source={controller.sourceRef} label={controller.label} />
        </div>)}
      </CreationGroup>
      <CreationGroup title="Input devices" addLabel="Add input device" empty={document.inputDevices.length === 0} form={(close) => <>
        <AddInputDevice document={document} onApplied={close} />
        <AvailableInputDevices document={document} />
      </>}>
        {document.inputDevices.map((device) => <div className="object-overview-row-with-actions" key={guiObjectKey(device.sourceRef)}>
          <SetupRow icon={<Keyboard aria-hidden="true" />} title={device.name} detail={countOf(device.controls.length, "control")} onOpen={() => void navigateToGuiObject(device.sourceRef)} />
          <MemberActions source={device.sourceRef} label={device.name} />
        </div>)}
      </CreationGroup>
    </main>
  );
}

/** A new controller's name, then its connection and outputs. */
function AddController({ newName, onApplied }: { newName: string; onApplied: () => void }) {
  const [name, setName] = useState(newName);
  const edit = useCallback((config: SetupControllerConfig, ports: SetupControllerPort[]): GuiEditCommand =>
    ({ type: "setup", edit: { type: "addController", config, ports, name: name.trim() } }), [name]);
  return <>
    <NewName name={name} onChange={setName} />
    <ControllerForm edit={edit} onApplied={onApplied} />
  </>;
}

/** A new input device's name, then its driver and controls. */
function AddInputDevice({ document, onApplied }: { document: SetupGuiDocument; onApplied: () => void }) {
  const [name, setName] = useState(document.newInputDeviceName);
  const edit = useCallback((driver: GuiInputDriver, controls: GuiInputControl[]): GuiEditCommand =>
    ({ type: "setup", edit: { type: "addInputDevice", driver, controls, name: name.trim() } }), [name]);
  return <>
    <NewName name={name} onChange={setName} />
    <InputDeviceForm keyCodes={document.keyCodes} limits={document.inputLimits} edit={edit} onApplied={onApplied} />
  </>;
}

function NewName({ name, onChange }: { name: string; onChange: (name: string) => void }) {
  return <Field label="Name"><TextField name="Name" live autoFocus value={name} invalid={name.trim() === ""} onCommit={onChange} /></Field>;
}

/** A group of the setup's members with the dialog that adds one: a "+" in its header, or in place of an empty list. */
function CreationGroup({ title, addLabel, empty, form, children }: { title: string; addLabel: string; empty: boolean; form: (close: () => void) => ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const show = () => { setOpen(true); };
  return <section className="object-overview-group">
    <div className="object-overview-group-header"><h3>{title}</h3>{!empty && <IconButton icon={Plus} label={addLabel} onClick={show} />}</div>
    {empty ? <AddRow label={addLabel} onAdd={show} /> : children}
    <Dialog open={open} onOpenChange={setOpen} title={addLabel} size="md">{open && form(() => { setOpen(false); })}</Dialog>
  </section>;
}

function SetupRow({ icon, title, detail, onOpen }: { icon: ReactNode; title: string; detail?: string; onOpen: () => void }) {
  return <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}><span className="object-overview-icon">{icon}</span><span className="object-overview-label"><strong>{title}</strong></span>{detail !== undefined && <span className="object-overview-detail">{detail}</span>}</a>;
}
