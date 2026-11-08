import { useRef, useState } from "react";
import { X } from "lucide-react";
import { DndContext, KeyboardSensor, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import { restrictToFirstScrollableAncestor, restrictToHorizontalAxis } from "@dnd-kit/modifiers";
import { SortableContext, arrayMove, horizontalListSortingStrategy, sortableKeyboardCoordinates, useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import { THEME_METRICS } from "../theme";
import type { EditorTab } from "../types";
import { runWorkspaceTransition } from "../workspaceTransitions";
import { IconButton, Tip } from "./kit";

const MODIFIERS = [restrictToHorizontalAxis, restrictToFirstScrollableAncestor];
const TRANSITION = { duration: THEME_METRICS.tabReorderDurationMs, easing: THEME_METRICS.tabReorderEasing };
// Enter opens a focused tab; Space picks it up to move with the arrow keys.
const KEYBOARD_CODES = { start: ["Space"], cancel: ["Escape"], end: ["Space", "Enter"] };

/** The open documents' tabs. Dragging one past the others moves it and makes it active. */
export function EditorTabStrip({ tabs, activePath }: { tabs: EditorTab[]; activePath: string | null }) {
  const [draggingPath, setDraggingPath] = useState<string | null>(null);
  // The dropped order, shown until the snapshot that carries it arrives.
  const [pendingOrder, setPendingOrder] = useState<string[] | null>(null);
  const moveGeneration = useRef(0);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: THEME_METRICS.interactionDragThreshold } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates, keyboardCodes: KEYBOARD_CODES })
  );
  const shown = pendingOrder === null
    ? tabs
    : pendingOrder.flatMap((path) => tabs.filter((tab) => tab.path === path));
  const paths = shown.map((tab) => tab.path);
  const highlighted = draggingPath ?? activePath;

  const onDragStart = ({ active }: DragStartEvent) => { setDraggingPath(String(active.id)); };
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setDraggingPath(null);
    const path = String(active.id);
    const from = paths.indexOf(path);
    const to = over === null ? from : paths.indexOf(String(over.id));
    const generation = ++moveGeneration.current;
    setPendingOrder(arrayMove(paths, from, to));
    void runSnapshotCommand(() => commands.moveTab(path, to)).finally(() => {
      if (moveGeneration.current === generation) setPendingOrder(null);
    });
  };
  const onDragCancel = () => {
    if (draggingPath !== null) void runSnapshotCommand(() => commands.openFile(draggingPath));
    setDraggingPath(null);
  };

  return (
    <div className="tab-strip">
      <DndContext sensors={sensors} collisionDetection={closestCenter} modifiers={MODIFIERS}
        onDragStart={onDragStart} onDragEnd={onDragEnd} onDragCancel={onDragCancel}>
        <SortableContext items={paths} strategy={horizontalListSortingStrategy}>
          {shown.map((tab) => <SortableTab key={tab.path} tab={tab} active={tab.path === highlighted} current={tab.path === activePath} />)}
        </SortableContext>
      </DndContext>
    </div>
  );
}

function SortableTab({ tab, active, current }: { tab: EditorTab; active: boolean; current: boolean }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: tab.path, transition: TRANSITION });
  return (
    <div
      ref={setNodeRef}
      className={`tab ${active ? "active" : ""} ${isDragging ? "dragging" : ""}`}
      style={{ transform: CSS.Translate.toString(transform), transition }}
    >
      <button
        type="button"
        className="tab-select"
        {...attributes}
        {...listeners}
        aria-current={current ? "page" : undefined}
        onClick={() => void runSnapshotCommand(() => commands.openFile(tab.path))}
      >
        <span>{tab.name}</span>
        <span className={tab.dirty ? "dirty-dot unsaved" : "dirty-dot"} aria-hidden="true" />
        {tab.externalState !== "current" && <Tip label={tab.externalState === "deletedOnDisk" ? "Deleted on disk" : "Changed on disk"}><span className="conflict-dot" /></Tip>}
      </button>
      <IconButton
        icon={X}
        className="tab-close"
        label={`Close ${tab.name}`}
        {...(current ? { command: "file.closeEditor" as const } : {})}
        onClick={(event) => {
          event.stopPropagation();
          void runWorkspaceTransition({ type: "closeFile", path: tab.path });
        }}
      />
    </div>
  );
}
