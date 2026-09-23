import { ControllerForm } from "../controller/ControllerForm";
import { Cable, Cpu, LayoutTemplate, MoreHorizontal } from "lucide-react";
import * as Dialog from "@radix-ui/react-dialog";
import { useState, type ReactNode } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { AvailableControllers, SetupControllerActions } from "./ControllerMembership";

export function SetupEditor({ document }: { document: SetupDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div><span className="object-overview-eyebrow">Display setup</span><h2>{document.objectKey}</h2></div>
        <span>{document.controllers.length} controllers</span>
      </header>
      <section className="object-overview-group">
        <h3>Composition</h3>
        <SetupRow icon={<LayoutTemplate aria-hidden="true" />} title="Layout" reference={referenceLabel(document.layoutRef)} detail="Fixture instances and groups" onOpen={() => void navigateToGuiObject(document.layoutRef)} />
        <SetupRow icon={<Cable aria-hidden="true" />} title="Patch" reference={referenceLabel(document.patchRef)} detail="LED output routes" onOpen={() => void navigateToGuiObject(document.patchRef)} />
        {(document.layoutReadOnly || document.patchReadOnly) && <button className="object-overview-inline-action" type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "copyLayout" }))}>Make independent layout copy</button>}
      </section>
      <section className="object-overview-group">
        <OverviewGroupHeader title="Controllers"><CreationDialog label="Add controller" title="New controller"><ControllerForm onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "addController", config, ports })); }} /><AvailableControllers document={document} /></CreationDialog></OverviewGroupHeader>
        {document.controllers.length === 0 ? <p className="object-overview-empty">No controllers attached.</p> : document.controllers.map((controller) => <div className="object-overview-row-with-actions" key={referenceLabel(controller.sourceRef)}><SetupRow icon={<Cpu aria-hidden="true" />} title={controller.label} reference={referenceLabel(controller.sourceRef)} detail={`${controller.ports.length} outputs`} onOpen={() => void navigateToGuiObject(controller.sourceRef)} /><details className="setup-controller-actions"><summary aria-label={`Setup actions for ${controller.label}`}><MoreHorizontal aria-hidden="true" /></summary><div><SetupControllerActions controller={controller} patchReadOnly={document.patchReadOnly} /></div></details></div>)}
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

function referenceLabel(reference: SetupDocument["layoutRef"]): string { return `${reference.path} · ${reference.objectKey}`; }
