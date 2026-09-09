import { useState } from "react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { GuiFixtureDefinition, SetupDocument } from "../../../types";
import { FixtureProfileForm } from "./FixtureProfileForm";

const newProfile = (): GuiFixtureDefinition => ({
  functions: [{ id: 1, name: "Dimmer", tag: "dimmer", kind: { type: "range" }, curve: { type: "linear" } }],
  channels: [{ slot: 0, role: { type: "coarse", function: 1 }, curve: { type: "linear" } }],
  behaviorRules: []
});

export function CreateFixtureProfile({ profiles }: { profiles: SetupDocument["fixtureProfiles"] }) {
  const [source, setSource] = useState<SetupDocument["fixtureProfiles"][number] | null>(null);
  const [name, setName] = useState("fixture");
  const [created, setCreated] = useState(false);
  if (created) return <p>Profile created. Select it when adding or editing a fixture element.</p>;
  return <>
    <label>Start from<select value={source === null ? -1 : profiles.findIndex((profile) => profile.id === source.id)} onChange={(event) => {
      const index = Number(event.target.value);
      const profile = index < 0 ? null : profiles[index];
      if (profile === undefined) throw new Error("Selected fixture profile is unavailable.");
      setSource(profile);
      setName(profile === null ? "fixture" : `${profile.name}_copy`);
    }}><option value={-1}>New profile</option>{profiles.map((profile, index) => <option key={profile.id} value={index}>Independent copy of {profile.name}</option>)}</select></label>
    <FixtureProfileForm key={source?.id ?? "new"} initial={source === null ? newProfile() : source.definition} onSave={async (definition, origin) => {
      await runGuiEditCommand((request) => commands.applySetupGuiEdit(request, { type: "createFixtureProfile", name, definition }), origin);
      setCreated(true);
    }}>
      <label>Profile identifier prefix<input required pattern="[A-Za-z_][A-Za-z0-9_]*" value={name} onChange={(event) => { setName(event.target.value); }} /></label>
    </FixtureProfileForm>
  </>;
}
