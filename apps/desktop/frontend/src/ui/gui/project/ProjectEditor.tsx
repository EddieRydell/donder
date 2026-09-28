import { OwnershipActions, ownershipLabel } from "../OwnershipActions";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Boxes, ListVideo } from "lucide-react";
import type { ReactNode } from "react";

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
        <span>{document.sequences.length} {document.sequences.length === 1 ? "sequence" : "sequences"}</span>
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
    </main>
  );
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
