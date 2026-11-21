import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Boxes, ListVideo, Play, Plus, Radio } from "lucide-react";
import { useState, type ReactNode } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import { AddRow, IconButton } from "../../kit";
import { PerformButton } from "../../PerformBar";
import { MemberActions, memberName } from "../MemberActions";
import { NameDialog } from "../NameDialog";

import type { GuiEditCommand, GuiObjectRef, ProjectGuiDocument } from "../../../types";
import { navigateToGuiObject, playSequence } from "../../../workspace/navigation";
import { countOf } from "../../count";

const addSequence = (name: string): GuiEditCommand =>
  ({ type: "project", edit: { type: "addSequence", initialColor: THEME_COLORS.defaultSequenceColor, name } });
const addConsole = (name: string): GuiEditCommand =>
  ({ type: "project", edit: { type: "addConsole", initialColor: THEME_COLORS.defaultSequenceColor, name } });

export function ProjectEditor({ document }: { document: ProjectGuiDocument }) {
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
        <h3>Display setup</h3>
        <div className="object-overview-row-with-actions"><ObjectRow
          icon={<Boxes aria-hidden="true" />}
          title="Display setup"
          onOpen={() => void navigateToGuiObject(document.setup)}
        /><MemberActions source={document.setup} label={memberName(document.setup)} deletable={false} /></div>
      </section>

      <MemberList title="Sequences" noun="sequence" members={document.sequences} newName={document.newSequenceName} add={addSequence}
        row={(sequence, name) => <><ObjectRow icon={<ListVideo aria-hidden="true" />} title={name} onOpen={() => void navigateToGuiObject(sequence)} />
          <IconButton icon={Play} label={`Play ${name}`} onClick={() => void playSequence(sequence)} /></>} />
      <MemberList title="Consoles" noun="console" members={document.consoles} newName={document.newConsoleName} add={addConsole}
        row={(console, name, index) => <><ObjectRow icon={<Radio aria-hidden="true" />} title={name} onOpen={() => void navigateToGuiObject(console)} />
          <PerformButton index={index} /></>} />
    </main>
  );
}

/** A list of the project's members with the "+" that names and adds one: in its header, or in place of an empty
 * list. */
function MemberList({ title, noun, members, newName, add, row }: {
  title: string;
  noun: string;
  members: GuiObjectRef[];
  newName: string;
  add: (name: string) => GuiEditCommand;
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
        const name = memberName(member);
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

function ObjectRow({ icon, title, onOpen }: { icon: ReactNode; title: string; onOpen: () => void }) {
  return (
    <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}>
      <span className="object-overview-icon">{icon}</span>
      <span className="object-overview-label"><strong>{title}</strong></span>
    </a>
  );
}
