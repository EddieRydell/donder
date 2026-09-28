import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronRight } from "lucide-react";
import { THEME_METRICS } from "../../../theme";

export type FixtureTool = "pixel" | "line" | "polyline" | "arc" | "circle" | "grid";
export const fixtureTools: { type: FixtureTool; label: string; instruction: string }[] = [
  { type: "pixel", label: "Pixel", instruction: "Click to place a pixel." },
  { type: "line", label: "Line", instruction: "Drag from the first endpoint to the last." },
  { type: "polyline", label: "Polyline", instruction: "Click corners, then press Enter to finish. Escape cancels." },
  { type: "arc", label: "Arc", instruction: "Drag from the center to the start of the arc. Adjust its end handle afterward." },
  { type: "circle", label: "Circle", instruction: "Drag from the center to the first pixel." },
  { type: "grid", label: "Grid", instruction: "Drag between opposite corners of the grid." }
];

export function FixtureContextMenu({ enabled, onTool, selected, onDuplicate, onDelete }: { enabled: boolean; onTool: (tool: FixtureTool) => void; selected: boolean; onDuplicate: () => void; onDelete: () => void }) {
  return <><ContextMenu.Sub>
    <ContextMenu.SubTrigger className="menu-item" disabled={!enabled}><span>Add shape</span><ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden /></ContextMenu.SubTrigger>
    <ContextMenu.Portal><ContextMenu.SubContent className="menu-content">
      {fixtureTools.map((tool) => <ContextMenu.Item className="menu-item" key={tool.type} disabled={!enabled} onSelect={() => { onTool(tool.type); }}>{tool.label}</ContextMenu.Item>)}
    </ContextMenu.SubContent></ContextMenu.Portal>
  </ContextMenu.Sub>
    {selected && <><ContextMenu.Separator className="menu-separator" /><ContextMenu.Item className="menu-item" disabled={!enabled} onSelect={onDuplicate}>Duplicate shape</ContextMenu.Item><ContextMenu.Item className="menu-item danger" disabled={!enabled} onSelect={onDelete}>Delete shape</ContextMenu.Item></>}
  </>;
}
