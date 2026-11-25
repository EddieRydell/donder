import { useMemo } from "react";

import type { SequenceGuiDocument } from "../../../types";

import { SequenceCanvas } from "./SequenceCanvas";
import { GraphEditorModal } from "./GraphEditorModal";
import { sequenceComposition } from "./composition";
import { useDocumentUi } from "../documentUi";

export function SequenceEditor({ document }: { document: SequenceGuiDocument }) {
  const graphOpen = useDocumentUi((ui) => ui.graphOpen);
  const composition = useMemo(() => sequenceComposition(document, {
    envelopes: document.clips.filter((clip) => clip.content.type === "envelope")
  }), [document]);
  return (
    <div className="sequence-editor">
      <SequenceCanvas document={document} />
      {graphOpen && <GraphEditorModal composition={composition} description="Sequence layers and operators" />}
    </div>
  );
}
