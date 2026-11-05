import { OwnershipActions } from "../OwnershipActions";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { ControllerForm } from "../controller/ControllerForm";
import { Cable, CircuitBoard, Keyboard, LayoutGrid, Plus } from "lucide-react";
import { InputDeviceForm } from "../inputDevice/InputDeviceForm";
import { AvailableInputDevices, inputDeviceEntries } from "./InputDeviceMembership";
import { useState, type ReactNode } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { AddRow, Dialog, IconButton } from "../../kit";
import { AvailableControllers, controllerEntries } from "./ControllerMembership";

export function SetupEditor({ document }: { document: SetupGuiDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div><span className="object-overview-eyebrow">Display setup</span><h2>{document.sourceRef.ownedPath.length > 0 ? "Display setup" : document.objectKey}</h2></div>
        <span>{document.controllers.length} controllers · {document.inputDevices.length} input devices</span>
      </header>
      <section className="object-overview-group">
        <h3>Composition</h3>
        <div className="object-overview-row-with-actions"><SetupRow icon={<LayoutGrid aria-hidden="true" />} title="Layout" onOpen={() => void navigateToGuiObject(document.layoutRef)} /><OwnershipActions declarations={document.availableDeclarations} source={document.layoutRef} label="Layout" /></div>
        <div className="object-overview-row-with-actions"><SetupRow icon={<Cable aria-hidden="true" />} title="Patch" onOpen={() => void navigateToGuiObject(document.patchRef)} /><OwnershipActions declarations={document.availableDeclarations} source={document.patchRef} label="Patch" /></div>
      </section>
      <CreationGroup title="Controllers" addLabel="Add controller" empty={document.controllers.length === 0} form={(close) => <>
        <ControllerForm onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "addController", config, ports } })); close(); }} />
        <AvailableControllers document={document} />
      </>}>
        {document.controllers.map((controller) => <div className="object-overview-row-with-actions" key={guiObjectKey(controller.sourceRef)}>
          <SetupRow icon={<CircuitBoard aria-hidden="true" />} title={controller.label} detail={`${controller.ports.length} outputs`} onOpen={() => void navigateToGuiObject(controller.sourceRef)} />
          <OwnershipActions declarations={document.availableDeclarations} source={controller.sourceRef} label={controller.label} entries={controllerEntries(controller)} />
        </div>)}
      </CreationGroup>
      <CreationGroup title="Input devices" addLabel="Add input device" empty={document.inputDevices.length === 0} form={(close) => <>
        <InputDeviceForm keyCodes={document.keyCodes} limits={document.inputLimits} onSave={async (driver, controls) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "addInputDevice", driver, controls } })); close(); }} />
        <AvailableInputDevices document={document} />
      </>}>
        {document.inputDevices.map((device) => <div className="object-overview-row-with-actions" key={guiObjectKey(device.sourceRef)}>
          <SetupRow icon={<Keyboard aria-hidden="true" />} title={device.name} detail={`${device.controls.length} controls`} onOpen={() => void navigateToGuiObject(device.sourceRef)} />
          <OwnershipActions declarations={document.availableDeclarations} source={device.sourceRef} label={device.name} entries={inputDeviceEntries(device)} />
        </div>)}
      </CreationGroup>
    </main>
  );
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
