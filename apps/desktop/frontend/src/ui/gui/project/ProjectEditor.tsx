import { FolderCog, ListVideo } from "lucide-react";
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
        <ObjectRow
          icon={<FolderCog aria-hidden="true" />}
          title={document.setup.objectKey}
          reference={`${document.setup.path} · setup`}
          detail="Layout, fixture instances and controls, patch, and controllers"
          onOpen={() => void navigateToGuiObject(document.setup)}
        />
      </section>

      <section className="object-overview-group">
        <div className="object-overview-heading">
          <h3>Sequences</h3>
          <span>{document.sequences.length}</span>
        </div>
        {document.sequences.length === 0 ? (
          <p className="object-overview-empty">No sequences are included in this project.</p>
        ) : document.sequences.map((sequence) => (
          <ObjectRow
            key={`${sequence.moduleId}:${sequence.path}:${sequence.objectKey}`}
            icon={<ListVideo aria-hidden="true" />}
            title={sequence.objectKey}
            reference={`${sequence.path} · sequence`}
            onOpen={() => void navigateToGuiObject(sequence)}
          />
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
