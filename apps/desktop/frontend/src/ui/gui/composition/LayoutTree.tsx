import { useState, type DragEvent } from "react";
import { isSecondaryClick } from "../../../platform";
import { ChevronDown, ChevronRight } from "lucide-react";
import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import { THEME_METRICS } from "../../../theme";
import type { GuiDocumentRequest, GuiLayoutFixture } from "../../../types";
import { descendants, layoutIndex, memberFixtures, membersOf, membershipCount, type LayoutItems } from "./layoutGraph";

type Location = { parent: number | null; before: number | null; row: number | null; position: "before" | "after" | "inside" | "root" };
/** A dragged row: the item and the group it was dragged out of. */
type Drag = { id: number; from: number | null; origin: GuiDocumentRequest; subtree: Set<number> };
type TreeProps = { selected: number[]; onSelect: (id: number, additive?: boolean) => void };
type DragControls = {
  enabled: boolean;
  hover: Location | null;
  start: (item: GuiLayoutFixture, from: number | null, event: DragEvent) => void;
  over: (location: Location, event: DragEvent) => boolean;
  drop: (location: Location, event: DragEvent) => void;
  end: () => void;
};
type TreeContext = { items: LayoutItems; pixelCounts: ReadonlyMap<number, number>; shared: (id: number) => number; controls: DragControls };

/**
 * The layout root and its groups. A member of several groups has a row under
 * each; every row is the same item. Dragging moves a row out of its group;
 * Alt-dragging adds the item to another group as well.
 */
export function LayoutTree({ fixtures, root, pixels, selected, onSelect, enabled, request, onError }: TreeProps & {
  fixtures: GuiLayoutFixture[]; root: number[]; pixels: { owner: number }[]; enabled: boolean; request: GuiDocumentRequest | null; onError: (error: unknown) => void;
}) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hover, setHover] = useState<Location | null>(null);
  const items = layoutIndex(fixtures);
  const children = (parent: number | null) => {
    if (parent === null) return root;
    const group = items.get(parent);
    return group === undefined ? [] : membersOf(group);
  };
  const valid = (location: Location, adding: boolean) => enabled && drag !== null
    && (location.parent === null || !drag.subtree.has(location.parent))
    && location.row !== drag.id
    && (location.parent === drag.from && !adding || !children(location.parent).includes(drag.id));
  const end = () => { setDrag(null); setHover(null); };
  const controls: DragControls = {
    enabled, hover,
    start: (item, from, event) => {
      event.stopPropagation();
      if (!enabled || request === null) { event.preventDefault(); return; }
      event.dataTransfer.effectAllowed = "copyMove";
      event.dataTransfer.setData("application/x-donder-layout-item", String(item.id));
      setDrag({ id: item.id, from, origin: request, subtree: new Set(descendants(items, item.id)) });
      onSelect(item.id);
    },
    over: (location, event) => {
      event.stopPropagation();
      if (!valid(location, event.altKey)) { event.dataTransfer.dropEffect = "none"; setHover(null); return false; }
      event.preventDefault();
      event.dataTransfer.dropEffect = event.altKey ? "copy" : "move";
      setHover((current) => current?.row === location.row && current.position === location.position && current.parent === location.parent && current.before === location.before ? current : location);
      return true;
    },
    drop: (location, event) => {
      event.preventDefault(); event.stopPropagation();
      if (valid(location, event.altKey) && drag !== null) {
        void runGuiEditCommand((current) => commands.applyLayoutGuiEdit(current, event.altKey
          ? { type: "addMember", id: drag.id, to: location.parent, before: location.before }
          : { type: "moveMember", id: drag.id, from: drag.from, to: location.parent, before: location.before }), drag.origin).catch(onError);
      }
      end();
    },
    end
  };
  const pixelCounts = new Map<number, number>();
  for (const pixel of pixels) pixelCounts.set(pixel.owner, (pixelCounts.get(pixel.owner) ?? 0) + 1);
  const context: TreeContext = { items, pixelCounts, shared: (id) => membershipCount(fixtures, root, id), controls };
  const top: Location = { parent: null, before: null, row: null, position: "root" };
  return <div className="composition-tree" data-dragging={drag !== null}
    onDragOver={(event) => { controls.over(top, event); }} onDrop={(event) => { controls.drop(top, event); }}
    onDragLeave={(event) => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setHover(null); }}>
    {root.length === 0 && <p className="composition-tree-empty">Right-click to add a group or fixture.</p>}
    <LayoutTreeRows members={root} parent={null} context={context} selected={selected} onSelect={onSelect} />
    {drag !== null && <div className="layout-tree-root-drop" data-drop={hover?.position === "root"}>Drop here to move to the top level (Alt to add)</div>}
  </div>;
}

function LayoutTreeRows({ members, parent, context, ...props }: TreeProps & { members: number[]; parent: number | null; context: TreeContext }) {
  return members.map((id, index) => {
    const item = context.items.get(id);
    return item === undefined ? null : <LayoutTreeItem key={id} item={item} parent={parent} next={members[index + 1] ?? null} context={context} {...props} />;
  });
}

function LayoutTreeItem({ item, parent, next, context, selected, onSelect }: TreeProps & {
  item: GuiLayoutFixture; parent: number | null; next: number | null; context: TreeContext;
}) {
  const { controls } = context;
  const isGroup = item.kind.type === "group";
  const [open, setOpen] = useState(isGroup);
  const fixtures = memberFixtures(context.items, item.id);
  const pixelCount = fixtures.reduce((total, fixture) => total + (context.pixelCounts.get(fixture) ?? 0), 0);
  const shared = context.shared(item.id);
  const metadata = item.kind.type === "group"
    ? `group · ${fixtures.length} fixtures · ${pixelCount} pixels`
    : `${item.kind.definition.type === "inline" ? "Owned" : "Linked"} · ${pixelCount} pixels · position ${item.kind.transform.position.xMeters}, ${item.kind.transform.position.yMeters}, ${item.kind.transform.position.zMeters}`;
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
    onToggle={(event) => { event.stopPropagation(); if (isGroup) setOpen(event.currentTarget.open); }} data-layout-item={item.id} data-layout-parent={parent ?? undefined}>
    <summary className={selected.includes(item.id) ? "selected" : ""} draggable={controls.enabled}
      data-drop={controls.hover?.row === item.id && controls.hover.parent === (controls.hover.position === "inside" ? item.id : parent) ? controls.hover.position : undefined}
      onDragStart={(event) => { controls.start(item, parent, event); }} onDragEnd={controls.end}
      onDragOver={(event) => { const target = location(event); if (controls.over(target, event) && target.position === "inside") setOpen(true); }}
      onDrop={(event) => { controls.drop(location(event), event); }}
      onClick={(event) => { if (isSecondaryClick(event)) { event.preventDefault(); return; } if (!isGroup || event.shiftKey || event.ctrlKey || event.metaKey) event.preventDefault(); onSelect(item.id, event.shiftKey || event.ctrlKey || event.metaKey); }}>
      <span className="composition-tree-icon" aria-hidden="true">{isGroup
        ? open ? <ChevronDown size={THEME_METRICS.iconSizeExtraSmall} /> : <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} />
        : null}</span>
      <span className="composition-tree-name">{item.name}</span><span className="composition-tree-meta">{metadata}</span>
      {shared > 1 && <span className="composition-tree-shared" title={`In ${shared} groups`}>{`×${shared}`}</span>}
    </summary>
    {item.kind.type === "group" && <div className="composition-tree-children"><LayoutTreeRows members={item.kind.members} parent={item.id} context={context} selected={selected} onSelect={onSelect} /></div>}
  </details>;
}
