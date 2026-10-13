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
        <span>{document.sequences.length} {document.sequences.length === 1 ? "sequence" : "sequences"} · {document.rigs.length} {document.rigs.length === 1 ? "rig" : "rigs"}</span>
      </header>

      <section className="object-overview-group">
        <h3>Display setup</h3>
        <div className="object-overview-row-with-actions"><ObjectRow
          icon={<Boxes aria-hidden="true" />}
          title="Display setup"
          reference={ownershipLabel(document.setup)}
          detail="Layout, fixture instances and controls, patch, and controllers"
          onOpen={() => void navigateToGuiObject(document.setup)}
        /><OwnershipActions sources={document.availableSources} source={document.setup} slot={{ type: "setup" }} label="Setup" /></div>
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
          /><OwnershipActions sources={document.availableSources} source={sequence} slot={{ type: "sequence", index }} label={sequence.ownedPath.length > 0 ? `Sequence ${index + 1}` : sequence.objectKey} /></div>
        ))}
      </section>

      <RigList document={document} />
    </main>
  );
}

/** The project's rigs, which Perform plays live. */
function RigList({ document }: { document: ProjectGuiDocument }) {
  const pending = useAppStore((state) => state.guiEditPending);
  const performing = usePerformStore((state) => state.perform?.session !== null && state.perform?.session !== undefined);
  return <section className="object-overview-group">
    <div className="object-overview-group-header">
      <h3>Rigs</h3>
      <button type="button" disabled={pending} onClick={() => void runGuiEditCommand((request) =>
        commands.applyGuiEdit(request, { type: "project", edit: { type: "addRig", initialColor: THEME_COLORS.defaultSequenceColor } }))}>Add rig</button>
    </div>
    {document.rigs.length === 0 ? (
      <p className="object-overview-empty">No rigs are included in this project.</p>
    ) : document.rigs.map((rig, index) => {
      const step = rig.ownedPath[rig.ownedPath.length - 1];
      const name = step?.type === "rig" ? step.name : rig.objectKey;
      return <div className="object-overview-row-with-actions" key={guiObjectKey(rig)}><ObjectRow
        icon={<Radio aria-hidden="true" />}
        title={name}
        reference={ownershipLabel(rig)}
        onOpen={() => void navigateToGuiObject(rig)}
      /><button type="button" className="object-overview-row-perform" disabled={performing} onClick={() => { startPerforming(index); }}>
        <Play aria-hidden="true" /> Perform
      </button><OwnershipActions sources={document.availableSources} source={rig} slot={{ type: "rig", index }} label={name} /></div>;
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
