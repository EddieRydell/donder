import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { GuiDocument } from "../../../types";
import { FixtureProfileForm } from "./FixtureProfileForm";

export function FixtureProfileEditor({ document }: { document: Extract<GuiDocument, { type: "fixtureProfile" }>["document"] }) {
  return <main className="setup-editor">
    <header className="object-overview-header"><div><span className="object-overview-eyebrow">{document.path}</span><h2>{document.objectKey}</h2></div></header>
    <section className="setup-section">
      <p>Changes apply to every fixture using this profile. Keep function and entry identifiers stable to preserve existing controls.</p>
      <FixtureProfileForm initial={document.definition} onSave={async (definition, origin) => {
        await runGuiEditCommand((request) => commands.applyGuiEdit(request, { type: "fixtureProfile", definition }), origin);
      }} />
    </section>
  </main>;
}
