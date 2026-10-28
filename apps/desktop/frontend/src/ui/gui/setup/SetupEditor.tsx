import { OwnershipActions, ownershipLabel } from "../OwnershipActions";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { ControllerForm } from "../controller/ControllerForm";
import { Cable, CircuitBoard, Keyboard, LayoutGrid } from "lucide-react";
import { InputDeviceForm } from "../inputDevice/InputDeviceForm";
import { AvailableInputDevices, SetupInputDeviceActions } from "./InputDeviceMembership";
import * as Dialog from "@radix-ui/react-dialog";
import { useState, type ReactNode } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupGuiDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { AvailableControllers, SetupControllerActions } from "./ControllerMembership";

export function SetupEditor({ document }: { document: SetupGuiDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div><span className="object-overview-eyebrow">Display setup</span><h2>{document.sourceRef.ownedPath.length > 0 ? "Display setup" : document.objectKey}</h2></div>
        <span>{document.controllers.length} controllers · {document.inputDevices.length} input devices</span>
      </header>
      <section className="object-overview-group">
        <h3>Composition</h3>
        <div className="object-overview-row-with-actions"><SetupRow icon={<LayoutGrid aria-hidden="true" />} title="Layout" reference={ownershipLabel(document.layoutRef)} detail="Fixture instances and groups" onOpen={() => void navigateToGuiObject(document.layoutRef)} /><OwnershipActions declarations={document.availableDeclarations} source={document.layoutRef} label="Layout" /></div>
        <div className="object-overview-row-with-actions"><SetupRow icon={<Cable aria-hidden="true" />} title="Patch" reference={ownershipLabel(document.patchRef)} detail="LED output routes" onOpen={() => void navigateToGuiObject(document.patchRef)} /><OwnershipActions declarations={document.availableDeclarations} source={document.patchRef} label="Patch" /></div>
      </section>
      <section className="object-overview-group">
        <OverviewGroupHeader title="Controllers"><CreationDialog label="Add controller" title="New controller"><ControllerForm onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "addController", config, ports } })); }} /><AvailableControllers document={document} /></CreationDialog></OverviewGroupHeader>
        {document.controllers.length === 0 ? <p className="object-overview-empty">No controllers attached.</p> : document.controllers.map((controller) => <div className="object-overview-row-with-actions" key={guiObjectKey(controller.sourceRef)}><SetupRow icon={<CircuitBoard aria-hidden="true" />} title={controller.label} reference={ownershipLabel(controller.sourceRef)} detail={`${controller.ports.length} outputs`} onOpen={() => void navigateToGuiObject(controller.sourceRef)} /><OwnershipActions declarations={document.availableDeclarations} source={controller.sourceRef} label={controller.label}><SetupControllerActions controller={controller} /></OwnershipActions></div>)}
      </section>
      <section className="object-overview-group">
        <OverviewGroupHeader title="Input devices"><CreationDialog label="Add input device" title="New input device"><InputDeviceForm keyCodes={document.keyCodes} limits={document.inputLimits} onSave={async (driver, controls) => { await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "setup", edit: { type: "addInputDevice", driver, controls } })); }} /><AvailableInputDevices document={document} /></CreationDialog></OverviewGroupHeader>
        {document.inputDevices.length === 0 ? <p className="object-overview-empty">No input devices attached.</p> : document.inputDevices.map((device) => <div className="object-overview-row-with-actions" key={guiObjectKey(device.sourceRef)}><SetupRow icon={<Keyboard aria-hidden="true" />} title={device.name} reference={ownershipLabel(device.sourceRef)} detail={`${device.controls.length} controls`} onOpen={() => void navigateToGuiObject(device.sourceRef)} /><OwnershipActions declarations={document.availableDeclarations} source={device.sourceRef} label={device.name}><SetupInputDeviceActions device={device} /></OwnershipActions></div>)}
      </section>
    </main>
  );
}

function OverviewGroupHeader({ title, children }: { title: string; children: ReactNode }) {
  return <div className="object-overview-group-header"><h3>{title}</h3>{children}</div>;
}

function CreationDialog({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return <Dialog.Root open={open} onOpenChange={setOpen}><Dialog.Trigger asChild><button className="object-overview-inline-action" type="button">{label}</button></Dialog.Trigger><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content setup-creation-dialog"><Dialog.Title>{title}</Dialog.Title>{children}<div className="dialog-actions"><Dialog.Close asChild><button type="button">Close</button></Dialog.Close></div></Dialog.Content></Dialog.Portal></Dialog.Root>;
}

function SetupRow({ icon, title, reference, detail, onOpen }: { icon: ReactNode; title: string; reference: string; detail: string; onOpen: () => void }) {
  return <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}><span className="object-overview-icon">{icon}</span><span className="object-overview-label"><strong>{title}</strong><span>{reference}</span></span><span className="object-overview-detail">{detail}</span></a>;
}

