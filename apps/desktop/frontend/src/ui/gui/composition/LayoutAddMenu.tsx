import { FolderPlus, Lightbulb, Plus } from "lucide-react";
import type { GuiObjectRef } from "../../../types";
import type { MenuEntry } from "../../kit";

type LayoutAdd = {
  availableFixtures: GuiObjectRef[];
  enabled: boolean;
  onAddFixture: (fixture: GuiObjectRef) => void;
  onCreateFixture: () => void;
  onAddGroup: () => void;
};

export function layoutAddEntries({ availableFixtures, enabled, onAddFixture, onCreateFixture, onAddGroup }: LayoutAdd): MenuEntry[] {
  return [
    { type: "submenu", label: "Add fixture", icon: Lightbulb, disabled: !enabled, entries: [
      ...availableFixtures.map((fixture): MenuEntry => ({ type: "action", label: `${fixture.objectKey} (${fixture.path})`, disabled: !enabled, onSelect: () => { onAddFixture(fixture); } })),
      ...(availableFixtures.length > 0 ? [{ type: "separator" } as const] : []),
      { type: "action", label: "New fixture", icon: Plus, disabled: !enabled, onSelect: onCreateFixture }
    ] },
    { type: "action", label: "Add group", icon: FolderPlus, disabled: !enabled, onSelect: onAddGroup }
  ];
}
