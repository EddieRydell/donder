import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronRight, Plus } from "lucide-react";
import { THEME_METRICS } from "../../../theme";
import type { GuiObjectRef } from "../../../types";

export function LayoutAddMenu({ availableFixtures, enabled, onAddFixture, onCreateFixture, onAddGroup }: {
  availableFixtures: GuiObjectRef[];
  enabled: boolean;
  onAddFixture: (fixture: GuiObjectRef) => void;
  onCreateFixture: () => void;
  onAddGroup: () => void;
}) {
  return <>
    <ContextMenu.Sub>
      <ContextMenu.SubTrigger className="menu-item" disabled={!enabled}><span>Add fixture</span><ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden /></ContextMenu.SubTrigger>
      <ContextMenu.Portal><ContextMenu.SubContent className="menu-content">
        {availableFixtures.map((fixture) => <ContextMenu.Item key={fixture.id} className="menu-item" disabled={!enabled} onSelect={() => { onAddFixture(fixture); }}>
          {fixture.objectKey} ({fixture.path})
        </ContextMenu.Item>)}
        {availableFixtures.length === 0 && <ContextMenu.Item className="menu-item" disabled>No existing fixtures available</ContextMenu.Item>}
        <ContextMenu.Separator className="menu-separator" />
        <ContextMenu.Item className="menu-item" disabled={!enabled} onSelect={onCreateFixture}><span>Create new fixture</span><Plus size={THEME_METRICS.iconSizeSmall} aria-hidden /></ContextMenu.Item>
      </ContextMenu.SubContent></ContextMenu.Portal>
    </ContextMenu.Sub>
    <ContextMenu.Item className="menu-item" disabled={!enabled} onSelect={onAddGroup}><span>Add group</span><Plus size={THEME_METRICS.iconSizeSmall} aria-hidden /></ContextMenu.Item>
  </>;
}
