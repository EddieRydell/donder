import { FixtureDefinitionFields } from "./FixtureDefinitionFields";
import { useState } from "react";
import type { Geometry, PropDocument } from "../../../types";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { InspectorScrollArea } from "../InspectorScrollArea";
import type { GuiFocus } from "../shared";

export function FixtureInspector({ document, selected }: { document: PropDocument; selected: GuiFocus }) {
  const fixture = document.fixture;
  return (
    <InspectorScrollArea>
      <h2>Fixture</h2>
          <label>Name<input readOnly value={fixture.name} /></label>
          <FixtureDefinitionForm key={JSON.stringify([fixture.geometry, fixture.bulbDiameterMeters])} fixture={fixture} />
          <p>{selected?.type === "point" ? `Point ${selected.index + 1}` : "Select a point."}</p>
    </InspectorScrollArea>
  );
}

function FixtureDefinitionForm({ fixture }: { fixture: PropDocument["fixture"] }) {
  const [geometry, setGeometry] = useState<Geometry>(fixture.geometry);
  const [diameter, setDiameter] = useState(fixture.bulbDiameterMeters);
  return <form className="setup-authoring-form" onSubmit={(event) => {
    event.preventDefault();
    void runGuiEditCommand((request) => commands.applyPropGuiEdit(request, { type: "updateDefinition", geometry, bulbDiameterMeters: diameter }));
  }}>
    <FixtureDefinitionFields geometry={geometry} diameter={diameter} onGeometryChange={setGeometry} onDiameterChange={setDiameter} />
    <button type="submit">Apply fixture</button>
  </form>;
}

