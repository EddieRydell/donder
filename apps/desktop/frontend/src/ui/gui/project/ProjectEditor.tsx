import { OwnershipActions, ownershipLabel } from "../OwnershipActions";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Boxes, ListVideo, Play, Radio } from "lucide-react";
import type { ReactNode } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand, useAppStore } from "../../../store";
import { THEME_COLORS } from "../../../theme";
import { startPerforming, usePerformStore } from "../../../perform";

import type { ProjectGuiDocument } from "../../../types";
import { navigateToGuiObject } from "../../../workspace/navigation";

export function ProjectEditor({ document }: { document: ProjectGuiDocument }) {
  return (
    <main className="project-overview">
      <header className="object-overview-header">
        <div>
          <span className="object-overview-eyebrow">Project</span>
          <h2>{document.objectKey}</h2>
        </div>
        <span>{document.sequences.length} {document.sequences.length === 1 ? "sequence" : "sequences"} · {document.consoles.length} {document.consoles.length === 1 ? "console" : "consoles"}</span>
      </header>

      <section className="object-overview-group">
        <h3>Display setup</h3>
        <div className="object-overview-row-with-actions"><ObjectRow
          icon={<Boxes aria-hidden="true" />}
          title="Display setup"
          reference={ownershipLabel(document.setup)}
          detail="Layout, fixture instances and controls, patch, and controllers"
          onOpen={() => void navigateToGuiObject(document.setup)}
        /><OwnershipActions declarations={document.availableDeclarations} source={document.setup} label="Setup" /></div>
      </section>

      <section className="object-overview-group">
        <div className="object-overview-heading">
          <h3>Sequences</h3>
          <span>{document.sequences.length}</span>
        </div>
        {document.sequences.length === 0 ? (
          <p className="object-overview-empty">No sequences are included in this project.</p>
        ) : document.sequences.map((sequence, index) => (
          <div className="object-overview-row-with-actions" key={guiObjectKey(sequence)}><ObjectRow
            icon={<ListVideo aria-hidden="true" />}
            title={sequence.ownedPath.length > 0 ? `Sequence ${index + 1}` : sequence.objectKey}
            reference={ownershipLabel(sequence)}
            onOpen={() => void navigateToGuiObject(sequence)}
          /><OwnershipActions declarations={document.availableDeclarations} source={sequence} label={sequence.ownedPath.length > 0 ? `Sequence ${index + 1}` : sequence.objectKey} /></div>
        ))}
      </section>

      <ConsoleList document={document} />
    </main>
  );
}

/** The project's consoles, which Perform plays live. */
function ConsoleList({ document }: { document: ProjectGuiDocument }) {
  const pending = useAppStore((state) => state.guiEditPending);
  const performing = usePerformStore((state) => state.perform?.session !== null && state.perform?.session !== undefined);
  return <section className="object-overview-group">
    <div className="object-overview-group-header">
      <h3>Consoles</h3>
      <button type="button" disabled={pending} onClick={() => void runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "project", edit: { type: "addConsole", initialColor: THEME_COLORS.defaultSequenceColor } }))}>Add console</button>
    </div>
    {document.consoles.length === 0 ? (
      <p className="object-overview-empty">No consoles are included in this project.</p>
    ) : document.consoles.map((entry, index) => {
      const step = entry.ownedPath[entry.ownedPath.length - 1];
      const name = step?.type === "console" ? step.name : entry.objectKey;
      return <div className="object-overview-row-with-actions" key={guiObjectKey(entry)}><ObjectRow
        icon={<Radio aria-hidden="true" />}
        title={name}
        reference={ownershipLabel(entry)}
        onOpen={() => void navigateToGuiObject(entry)}
      /><button type="button" className="object-overview-row-perform" disabled={performing} onClick={() => { startPerforming(index); }}>
        <Play aria-hidden="true" /> Perform
      </button><OwnershipActions declarations={document.availableDeclarations} source={entry} label={name} /></div>;
    })}
  </section>;
}

function ObjectRow({
  icon,
  title,
  reference,
  detail,
  onOpen
}: {
  icon: ReactNode;
  title: string;
  reference: string;
  detail?: string;
  onOpen: () => void;
}) {
  return (
    <a href="#" className="object-overview-row" onClick={(event) => { event.preventDefault(); onOpen(); }}>
      <span className="object-overview-icon">{icon}</span>
      <span className="object-overview-label">
        <strong>{title}</strong>
        <span>{reference}</span>
      </span>
      {detail !== undefined && <span className="object-overview-detail">{detail}</span>}
    </a>
  );
}
