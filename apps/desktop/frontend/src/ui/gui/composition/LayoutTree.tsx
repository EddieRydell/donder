import { useState, type DragEvent } from "react";
import { isSecondaryClick } from "../../../platform";
import { ChevronDown, ChevronRight } from "lucide-react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, GuiLayoutFixture } from "../../../types";

type Location = { parent: number | null; before: number | null; row: number | null; position: "before" | "after" | "inside" | "root" };
type Drag = { id: number; origin: GuiDocumentRequest; subtree: Set<number> };
type TreeProps = { items: GuiLayoutFixture[]; pixels: { owner: number }[]; selected: number[]; onSelect: (id: number, additive?: boolean) => void };
type DragControls = {
  enabled: boolean;
  hover: Location | null;
  start: (item: GuiLayoutFixture, event: DragEvent) => void;
  over: (location: Location, event: DragEvent) => boolean;
  drop: (location: Location, event: DragEvent) => void;
  end: () => void;
};

export function LayoutTree({ items, pixels, selected, onSelect, enabled, request, onError }: TreeProps & {
  enabled: boolean; request: GuiDocumentRequest | null; onError: (error: unknown) => void;
}) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hover, setHover] = useState<Location | null>(null);
  const valid = (location: Location) => enabled && drag !== null
    && (location.parent === null || !drag.subtree.has(location.parent))
    && location.row !== drag.id;
  const end = () => { setDrag(null); setHover(null); };
  const controls: DragControls = {
    enabled, hover,
    start: (item, event) => {
      event.stopPropagation();
      if (!enabled || request === null) { event.preventDefault(); return; }
      const subtree = new Set<number>();
      const visit = (item: GuiLayoutFixture) => { subtree.add(item.id); if (item.kind.type === "group") item.kind.children.forEach(visit); };
      visit(item);
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("application/x-donder-layout-item", String(item.id));
      setDrag({ id: item.id, origin: request, subtree });
      onSelect(item.id);
    },
    over: (location, event) => {
      event.stopPropagation();
      if (!valid(location)) { event.dataTransfer.dropEffect = "none"; setHover(null); return false; }
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      setHover((current) => current?.row === location.row && current.position === location.position && current.parent === location.parent && current.before === location.before ? current : location);
      return true;
    },
    drop: (location, event) => {
      event.preventDefault(); event.stopPropagation();
      if (valid(location) && drag !== null) {
        void runGuiEditCommand((current) => commands.applyLayoutGuiEdit(current, {
          type: "reparentFixture", id: drag.id, parent: location.parent, before: location.before
        }), drag.origin).catch(onError);
      }
      end();
    },
    end
  };
  const root: Location = { parent: null, before: null, row: null, position: "root" };
  return <div className="composition-tree" data-dragging={drag !== null}
    onDragOver={(event) => { controls.over(root, event); }} onDrop={(event) => { controls.drop(root, event); }}
    onDragLeave={(event) => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setHover(null); }}>
    {items.length === 0 && <p className="composition-tree-empty">Right-click to add a group or fixture.</p>}
    {items.map((item, index) => <LayoutTreeItem key={item.id} item={item} parent={null} next={items[index + 1]?.id ?? null} pixels={pixels} selected={selected} onSelect={onSelect} controls={controls} />)}
    {drag !== null && <div className="layout-tree-root-drop" data-drop={hover?.position === "root"}>Drop here to move to the top level</div>}
  </div>;
}

function LayoutTreeItem({ item, parent, next, pixels, selected, onSelect, controls }: Omit<TreeProps, "items"> & {
  item: GuiLayoutFixture; parent: number | null; next: number | null; controls: DragControls;
}) {
  const isGroup = item.kind.type === "group";
  const [open, setOpen] = useState(isGroup);
  const pixelCount = countPixels(item, pixels);
  const metadata = item.kind.type === "group"
    ? `group \u00b7 ${countFixtures(item.kind.children)} fixtures \u00b7 ${pixelCount} pixels`
    : `${item.kind.definition.type === "inline" ? "Owned" : "Linked"} \u00b7 ${pixelCount} pixels \u00b7 position ${item.kind.transform.position.xMeters}, ${item.kind.transform.position.yMeters}, ${item.kind.transform.position.zMeters}`;
  const location = (event: DragEvent<HTMLElement>): Location => {
    const rect = event.currentTarget.getBoundingClientRect();
    const y = (event.clientY - rect.top) / rect.height;
    const edge = THEME_METRICS.layoutTreeDropEdge;
    if (isGroup && y >= edge && y <= 1 - edge) return { parent: item.id, before: null, row: item.id, position: "inside" };
    return y < (isGroup ? edge : 0.5)
      ? { parent, before: item.id, row: item.id, position: "before" }
      : { parent, before: next, row: item.id, position: "after" };
  };
  return <details className="composition-tree-item" open={isGroup ? open : undefined}
    onToggle={(event) => { if (isGroup) setOpen(event.currentTarget.open); }} data-layout-item={item.id}>
    <summary className={selected.includes(item.id) ? "selected" : ""} draggable={controls.enabled}
      data-drop={controls.hover?.row === item.id ? controls.hover.position : undefined}
      onDragStart={(event) => { controls.start(item, event); }} onDragEnd={controls.end}
      onDragOver={(event) => { const target = location(event); if (controls.over(target, event) && target.position === "inside") setOpen(true); }}
      onDrop={(event) => { controls.drop(location(event), event); }}
      onClick={(event) => { if (isSecondaryClick(event)) { event.preventDefault(); return; } if (!isGroup || event.shiftKey || event.ctrlKey || event.metaKey) event.preventDefault(); onSelect(item.id, event.shiftKey || event.ctrlKey || event.metaKey); }}>
      <span className="composition-tree-icon" aria-hidden="true">{isGroup
        ? open ? <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} /> : <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} />
        : null}</span>
      <span className="composition-tree-name">{item.name}</span><span className="composition-tree-meta">{metadata}</span>
    </summary>
    {item.kind.type === "group" && <div className="composition-tree-children">{item.kind.children.map((child, index) => <LayoutTreeItem key={child.id} item={child} parent={item.id} next={item.kind.type === "group" ? item.kind.children[index + 1]?.id ?? null : null} pixels={pixels} selected={selected} onSelect={onSelect} controls={controls} />)}</div>}
  </details>;
}

function countFixtures(items: GuiLayoutFixture[]): number {
  return items.reduce((total, item) => total + (item.kind.type === "group" ? countFixtures(item.kind.children) : 1), 0);
}
function countPixels(item: GuiLayoutFixture, pixels: { owner: number }[]): number {
  return item.kind.type === "group" ? item.kind.children.reduce((total, child) => total + countPixels(child, pixels), 0) : pixels.filter((pixel) => pixel.owner === item.id).length;
}
