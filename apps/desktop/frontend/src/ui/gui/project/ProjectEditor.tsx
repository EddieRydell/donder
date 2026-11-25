import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Cable, CircuitBoard, Keyboard, LayoutGrid, ListVideo, Play, Plus, Radio } from "lucide-react";
import { useCallback, useState, type ReactNode } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import { AddRow, Dialog, Field, IconButton, TextField } from "../../kit";
import { PerformButton } from "../../PerformBar";
import { MemberActions } from "../MemberActions";
import { NameDialog } from "../NameDialog";
import { ControllerForm } from "../controller/ControllerForm";
import { InputDeviceForm } from "../inputDevice/InputDeviceForm";
import { PatchEditor } from "../patch/PatchEditor";

import type { GuiEdit, GuiInputControl, GuiInputDriver, GuiObjectRef, ProjectGuiDocument, SetupControllerConfig, SetupControllerPort } from "../../../types";
import { navigateToGuiObject, playSequence } from "../../../workspace/navigation";
import { countOf } from "../../count";

const addSequence = (name: string): GuiEdit =>
  ({ type: "project", edit: { type: "addSequence", initialColor: THEME_COLORS.defaultSequenceColor, name } });
const addConsole = (name: string): GuiEdit =>
  ({ type: "project", edit: { type: "addConsole", initialColor: THEME_COLORS.defaultSequenceColor, name } });

/** The project's overview: its layout and outputs, controllers, input devices, sequences and consoles. */
export function ProjectEditor({ document }: { document: ProjectGuiDocument }) {
  const [outputs, setOutputs] = useState(false);
  if (outputs) return <PatchEditor document={document.outputs} onClose={() => { setOutputs(false); }} />;
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div>
          <span className="object-overview-eyebrow">Project</span>
          <h2>{document.objectKey}</h2>
        </div>
        <span>{countOf(document.sequences.length, "sequence")} · {countOf(document.consoles.length, "console")}</span>
      </header>

      <section className="object-overview-group">
        <h3>Display</h3>
        <div className="object-overview-row-with-actions"><ObjectRow icon={<LayoutGrid aria-hidden="true" />} title="Layout" onOpen={() => void navigateToGuiObject(document.layoutRef)} />
          <MemberActions source={document.layoutRef} label={document.layoutRef.objectKey} deletable={false} /></div>
        <ObjectRow icon={<Cable aria-hidden="true" />} title="Outputs" detail={countOf(document.outputs.routes.length, "output")} onOpen={() => { setOutputs(true); }} />
      </section>
      <CreationGroup title="Controllers" addLabel="Add controller" empty={document.controllers.length === 0} form={(close) => <AddController newName={document.newControllerName} onApplied={close} />}>
        {document.controllers.map((controller) => <div className="object-overview-row-with-actions" key={guiObjectKey(controller.sourceRef)}>
          <ObjectRow icon={<CircuitBoard aria-hidden="true" />} title={controller.label} detail={controller.config.type === "donder" ? countOf(controller.ports.length, "output") : countOf(controller.config.universes.count, "universe")} onOpen={() => void navigateToGuiObject(controller.sourceRef)} />
          <MemberActions source={controller.sourceRef} label={controller.label} />
        </div>)}
      </CreationGroup>
      <CreationGroup title="Input devices" addLabel="Add input device" empty={document.inputDevices.length === 0} form={(close) => <AddInputDevice document={document} onApplied={close} />}>
        {document.inputDevices.map((device) => <div className="object-overview-row-with-actions" key={guiObjectKey(device.sourceRef)}>
          <ObjectRow icon={<Keyboard aria-hidden="true" />} title={device.name} detail={countOf(device.controls.length, "control")} onOpen={() => void navigateToGuiObject(device.sourceRef)} />
          <MemberActions source={device.sourceRef} label={device.name} />
        </div>)}
      </CreationGroup>

      <MemberList title="Sequences" noun="sequence" members={document.sequences} newName={document.newSequenceName} add={addSequence}
        row={(sequence, name) => <><ObjectRow icon={<ListVideo aria-hidden="true" />} title={name} onOpen={() => void navigateToGuiObject(sequence)} />
          <IconButton icon={Play} label={`Play ${name}`} onClick={() => void playSequence(sequence)} /></>} />
      <MemberList title="Consoles" noun="console" members={document.consoles} newName={document.newConsoleName} add={addConsole}
        row={(console, name, index) => <><ObjectRow icon={<Radio aria-hidden="true" />} title={name} onOpen={() => void navigateToGuiObject(console)} />
          <PerformButton index={index} /></>} />
    </main>
  );
}

/** A new controller's name, then its connection and outputs. */
function AddController({ newName, onApplied }: { newName: string; onApplied: () => void }) {
  const [name, setName] = useState(newName);
  const edit = useCallback((config: SetupControllerConfig, ports: SetupControllerPort[]): GuiEdit =>
    ({ type: "project", edit: { type: "addController", config, ports, name: name.trim() } }), [name]);
  return <>
    <NewName name={name} onChange={setName} />
    <ControllerForm edit={edit} onApplied={onApplied} />
  </>;
}

/** A new input device's name, then its driver and controls. */
function AddInputDevice({ document, onApplied }: { document: ProjectGuiDocument; onApplied: () => void }) {
  const [name, setName] = useState(document.newInputDeviceName);
  const edit = useCallback((driver: GuiInputDriver, controls: GuiInputControl[]): GuiEdit =>
    ({ type: "project", edit: { type: "addInputDevice", driver, controls, name: name.trim() } }), [name]);
  return <>
    <NewName name={name} onChange={setName} />
    <InputDeviceForm keyCodes={document.keyCodes} limits={document.inputLimits} edit={edit} onApplied={onApplied} />
  </>;
}

function NewName({ name, onChange }: { name: string; onChange: (name: string) => void }) {
  return <Field label="Name"><TextField name="Name" live autoFocus value={name} invalid={name.trim() === ""} onCommit={onChange} /></Field>;
}

/** A group of the project's controllers or input devices with the dialog that adds one: a "+" in its header, or in
 * place of an empty list. */
function CreationGroup({ title, addLabel, empty, form, children }: { title: string; addLabel: string; empty: boolean; form: (close: () => void) => ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const show = () => { setOpen(true); };
  return <section className="object-overview-group">
    <div className="object-overview-group-header"><h3>{title}</h3>{!empty && <IconButton icon={Plus} label={addLabel} onClick={show} />}</div>
    {empty ? <AddRow label={addLabel} onAdd={show} /> : children}
    <Dialog open={open} onOpenChange={setOpen} title={addLabel} size="md">{open && form(() => { setOpen(false); })}</Dialog>
  </section>;
}

/** A list of the project's members with the "+" that names and adds one: in its header, or in place of an empty
 * list. */
function MemberList({ title, noun, members, newName, add, row }: {
  title: string;
  noun: string;
  members: GuiObjectRef[];
  newName: string;
  add: (name: string) => GuiEdit;
  row: (member: GuiObjectRef, name: string, index: number) => ReactNode;
}) {
  const pending = useAppStore((state) => state.guiEditPending);
  const [adding, setAdding] = useState(false);
  const label = `Add ${noun}`;
  const show = () => { setAdding(true); };
  return <section className="object-overview-group">
    <div className="object-overview-group-header">
      <h3>{title}</h3>
      {members.length > 0 && <IconButton icon={Plus} label={label} disabled={pending} onClick={show} />}
    </div>
    {members.length === 0 ? <AddRow label={label} disabled={pending} onAdd={show} />
      : members.map((member, index) => {
        const name = member.objectKey;
        return <div className="object-overview-row-with-actions" key={guiObjectKey(member)}>
          {row(member, name, index)}
          <MemberActions source={member} label={name} />
        </div>;
      })}
    {adding && <NameDialog title={label} action="Add" initial={newName} draft={add}
      submit={(name) => runGuiEditCommand((request) => commands.applyGuiEdit(request, add(name)))}
      onClose={() => { setAdding(false); }} />}
  </section>;
}

function ObjectRow({ icon, title, detail, onOpen }: { icon: ReactNode; title: string; detail?: string; onOpen: () => void }) {
  return (
    <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}>
      <span className="object-overview-icon">{icon}</span>
      <span className="object-overview-label"><strong>{title}</strong></span>
      {detail !== undefined && <span className="object-overview-detail">{detail}</span>}
    </a>
  );
}
