import { CircleDashed, Copy, Dot, Grid3x3, Rainbow, Slash, Trash2, Waypoints, type LucideIcon } from "lucide-react";
import type { MenuEntry } from "../../kit";

export type FixtureTool = "pixel" | "line" | "polyline" | "arc" | "circle" | "grid";
/** The drawing tools; `instruction` is the tool's tooltip. */
export const fixtureTools: { type: FixtureTool; label: string; icon: LucideIcon; instruction: string }[] = [
  { type: "pixel", label: "Pixel", icon: Dot, instruction: "Click to place a pixel." },
  { type: "line", label: "Line", icon: Slash, instruction: "Drag from the first endpoint to the last." },
  { type: "polyline", label: "Polyline", icon: Waypoints, instruction: "Click corners, then press Enter to finish. Escape cancels." },
  { type: "arc", label: "Arc", icon: Rainbow, instruction: "Drag from the center to the start of the arc. Adjust its end handle afterward." },
  { type: "circle", label: "Circle", icon: CircleDashed, instruction: "Drag from the center to the first pixel." },
  { type: "grid", label: "Grid", icon: Grid3x3, instruction: "Drag between opposite corners of the grid." }
];

type FixtureMenu = { enabled: boolean; onTool: (tool: FixtureTool) => void; selected: boolean; onDuplicate: () => void; onDelete: () => void };

export function fixtureMenuEntries({ enabled, onTool, selected, onDuplicate, onDelete }: FixtureMenu): MenuEntry[] {
  return [
    { type: "submenu", label: "Add shape", disabled: !enabled, entries: fixtureTools.map((tool): MenuEntry => ({ type: "action", label: tool.label, icon: tool.icon, disabled: !enabled, onSelect: () => { onTool(tool.type); } })) },
    ...(selected ? [
      { type: "separator" },
      { type: "action", label: "Duplicate", icon: Copy, shortcut: "edit.duplicate", disabled: !enabled, onSelect: onDuplicate },
      { type: "action", label: "Delete", icon: Trash2, shortcut: "edit.delete", danger: true, disabled: !enabled, onSelect: onDelete }
    ] satisfies MenuEntry[] : [])
  ];
}
