import { ControllerForm } from "../controller/ControllerForm";
import { Boxes, Cable, Cpu, LayoutTemplate, Lightbulb, SlidersHorizontal } from "lucide-react";
import * as Dialog from "@radix-ui/react-dialog";
import { useState, type ReactNode } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";
import { AssignOutputForm, OutputAssignments } from "./SetupAuthoring";
import { AvailableControllers, ControllerMembership } from "./ControllerMembership";
import { CreateFixtureProfile } from "../fixtureProfile/CreateFixtureProfile";
import { ControlOutputForm } from "./ControlOutputForm";
import { FixtureOutputForm } from "./FixtureOutputForm";
import { OutputTestForm } from "./OutputTestForm";

export function SetupEditor({ document }: { document: SetupDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div><span className="object-overview-eyebrow">Display setup</span><h2>{document.objectKey}</h2></div>
        <span>{document.elements.length} elements · {document.controllers.length} controllers</span>
      </header>
      <section className="object-overview-group">
        <h3>Composition</h3>
        <SetupRow icon={<LayoutTemplate aria-hidden="true" />} title="Layout" reference={referenceLabel(document.previewRef)} detail={`${document.previewLinks.length} placements`} onOpen={() => void navigateToGuiObject(document.previewRef)} />
        <SetupRow icon={<Boxes aria-hidden="true" />} title="Fixture instances & controls" reference={referenceLabel(document.elementsRef)} detail={`${document.elements.length} fixture instances and controls`} onOpen={() => void navigateToGuiObject(document.elementsRef)} />
        <SetupRow icon={<Cable aria-hidden="true" />} title="Patch" reference={referenceLabel(document.patchRef)} detail={`${document.outputAssignments.length} assigned outputs`} onOpen={() => void navigateToGuiObject(document.patchRef)} />
        {(document.elementsReadOnly || document.previewReadOnly || document.patchReadOnly) && <button type="button" onClick={() => void runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "copyLayout" }))}>Make independent layout copy</button>}
        <CreationDialog label="Assign outputs" title="Setup outputs"><OutputTestForm document={document} /><AssignOutputForm document={document} /><FixtureOutputForm document={document} /><ControlOutputForm document={document} /><OutputAssignments document={document} /></CreationDialog>
      </section>
      <section className="object-overview-group">
        <h3>Fixture instances</h3>
        {document.previewLinks.length === 0 ? <p className="object-overview-empty">No fixture instances in this layout.</p> : document.previewLinks.map((link) => <SetupRow key={link.propId} icon={<Lightbulb aria-hidden="true" />} title={link.name} reference={`Fixture definition · ${referenceLabel(link.definitionRef)}`} detail={`${link.pointCount} points`} onOpen={() => void navigateToGuiObject(link.definitionRef)} />)}
      </section>
      <section className="object-overview-group">
        <OverviewGroupHeader title="Controllers"><CreationDialog label="Add controller" title="New controller"><ControllerForm onSave={async (config, ports) => { await runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "addController", config, ports })); }} /><AvailableControllers document={document} /></CreationDialog></OverviewGroupHeader>
        {document.controllers.length === 0 ? <p className="object-overview-empty">No controllers attached.</p> : document.controllers.map((controller) => <div key={referenceLabel(controller.sourceRef)}><SetupRow icon={<Cpu aria-hidden="true" />} title={controller.label} reference={referenceLabel(controller.sourceRef)} detail={`${controller.ports.length} outputs`} onOpen={() => void navigateToGuiObject(controller.sourceRef)} /><CreationDialog label="Setup membership" title={controller.label}><ControllerMembership controller={controller} patchReadOnly={document.patchReadOnly} /></CreationDialog></div>)}
      </section>
      <section className="object-overview-group">
        <OverviewGroupHeader title="Fixture profiles"><CreationDialog label="Add fixture profile" title="New fixture profile"><CreateFixtureProfile profiles={document.fixtureProfiles} /></CreationDialog></OverviewGroupHeader>
        {document.fixtureProfiles.length === 0 ? <p className="object-overview-empty">No fixture profiles.</p> : document.fixtureProfiles.map((profile) => <SetupRow key={profile.id} icon={<SlidersHorizontal aria-hidden="true" />} title={profile.name} reference={referenceLabel(profile.sourceRef)} detail={`${profile.channelCount} channels`} onOpen={() => void navigateToGuiObject(profile.sourceRef)} />)}
      </section>
    </main>
  );
}

function OverviewGroupHeader({ title, children }: { title: string; children: ReactNode }) {
  return <div className="object-overview-group-header"><h3>{title}</h3>{children}</div>;
}

function CreationDialog({ label, title, children }: { label: string; title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return <Dialog.Root open={open} onOpenChange={setOpen}><Dialog.Trigger asChild><button type="button">{label}</button></Dialog.Trigger><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content setup-creation-dialog"><Dialog.Title>{title}</Dialog.Title>{children}<div className="dialog-actions"><Dialog.Close asChild><button type="button">Close</button></Dialog.Close></div></Dialog.Content></Dialog.Portal></Dialog.Root>;
}

function SetupRow({ icon, title, reference, detail, onOpen }: { icon: ReactNode; title: string; reference: string; detail: string; onOpen: () => void }) {
  return <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}><span className="object-overview-icon">{icon}</span><span className="object-overview-label"><strong>{title}</strong><span>{reference}</span></span><span className="object-overview-detail">{detail}</span></a>;
}

function referenceLabel(reference: SetupDocument["previewRef"]): string { return `${reference.path} · ${reference.objectKey}`; }
