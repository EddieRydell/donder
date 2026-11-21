import { OwnershipActions } from "../OwnershipActions";
import { guiObjectKey, guiObjectName } from "../../../workspace/guiIdentity";
import { Boxes, ListVideo, Play, Plus, Radio } from "lucide-react";
import type { ReactNode } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import { AddRow, IconButton } from "../../kit";
import { PerformButton } from "../../PerformBar";
import { DEFAULT_OBJECT_STORAGE } from "../objectStorage";

import type { ProjectGuiDocument } from "../../../types";
import { navigateToGuiObject, playSequence } from "../../../workspace/navigation";
import { countOf } from "../../count";

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
        /><OwnershipActions declarations={document.availableDeclarations} source={document.setup} label="Setup" /></div>
      </section>

      <SequenceList document={document} />
      <ConsoleList document={document} />
    </main>
  );
}

/** The project's sequences. */
function SequenceList({ document }: { document: ProjectGuiDocument }) {
  const pending = useAppStore((state) => state.guiEditPending);
  const addSequence = () => void runGuiEditCommand((request) =>
    commands.applyGuiEdit(request, { type: "project", edit: { type: "addSequence", initialColor: THEME_COLORS.defaultSequenceColor, storage: DEFAULT_OBJECT_STORAGE } }));
  return <section className="object-overview-group">
    <div className="object-overview-group-header">
      <h3>Sequences</h3>
      {document.sequences.length > 0 && <IconButton icon={Plus} label="Add sequence" disabled={pending} onClick={addSequence} />}
    </div>
    {document.sequences.length === 0 ? <AddRow label="Add sequence" disabled={pending} onAdd={addSequence} />
      : document.sequences.map((sequence) => {
        const name = guiObjectName(sequence) ?? sequence.objectKey;
        return <div className="object-overview-row-with-actions" key={guiObjectKey(sequence)}><ObjectRow
          icon={<ListVideo aria-hidden="true" />}
          title={name}
          onOpen={() => void navigateToGuiObject(sequence)}
        /><IconButton icon={Play} label={`Play ${name}`} onClick={() => void playSequence(sequence)} />
          <OwnershipActions declarations={document.availableDeclarations} source={sequence} label={name}
            entries={[{ type: "action", label: "Play", icon: Play, onSelect: () => void playSequence(sequence) }]} /></div>;
      })}
  </section>;
}

/** The project's consoles, which Perform plays live. */
function ConsoleList({ document }: { document: ProjectGuiDocument }) {
  const pending = useAppStore((state) => state.guiEditPending);
  const addConsole = () => void runGuiEditCommand((request) =>
    commands.applyGuiEdit(request, { type: "project", edit: { type: "addConsole", initialColor: THEME_COLORS.defaultSequenceColor, storage: DEFAULT_OBJECT_STORAGE } }));
  return <section className="object-overview-group">
    <div className="object-overview-group-header">
      <h3>Consoles</h3>
      {document.consoles.length > 0 && <IconButton icon={Plus} label="Add console" disabled={pending} onClick={addConsole} />}
    </div>
    {document.consoles.length === 0 ? <AddRow label="Add console" disabled={pending} onAdd={addConsole} />
      : document.consoles.map((entry, index) => {
        const name = guiObjectName(entry) ?? entry.objectKey;
        return <div className="object-overview-row-with-actions" key={guiObjectKey(entry)}><ObjectRow
          icon={<Radio aria-hidden="true" />}
          title={name}
          onOpen={() => void navigateToGuiObject(entry)}
        /><PerformButton index={index} />
          <OwnershipActions declarations={document.availableDeclarations} source={entry} label={name} /></div>;
      })}
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
