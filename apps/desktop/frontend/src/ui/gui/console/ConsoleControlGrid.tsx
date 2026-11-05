import { Flag, Play, Plus, Repeat, SlidersHorizontal, Square, X } from "lucide-react";
import { useMemo } from "react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { ConsoleAction, ConsoleEventKind, ConsoleGuiDocument, ConsoleInputControl, ConsoleInputDevice, ConsoleOn, ConsolePlayMode, GuiEditUpdate, SequenceEffectDefinition } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { Badge, Button, IconButton, MenuButton, type MenuEntry } from "../../kit";
import { definitionTree, type DefinitionFolder } from "../sequence/definitionMenu";
import type { GuiFocus } from "../shared";
import { answers, applyConsoleEdit, controlRef, focusOnEffect, inputLabel, targetLabel } from "./consoleEditing";

type Entry = { on: ConsoleOn; index: number; action: ConsoleAction };

/** The effect an action plays, starts, stops or toggles, or the live collection or automation it names. */
function actionTarget(document: ConsoleGuiDocument, action: ConsoleAction): string {
  switch (action.type) {
    case "play":
    case "start":
    case "stop":
    case "toggle": return document.clips.find((clip) => clip.id === action.clipId)?.name ?? "Missing effect";
    case "mark": return action.into;
    case "set": return targetLabel(document, action.target);
  }
}

/** What an action does beyond naming its target: copies and length of a played effect. */
function actionDetail(action: ConsoleAction): string | null {
  if (action.type !== "play") return null;
  const length = action.length.type === "fixed" ? `${action.length.durationSeconds} s` : "length by touch";
  return action.copies > 1 ? `×${action.copies} · ${length}` : length;
}

function ActionIcon({ action }: { action: ConsoleAction }) {
  const size = THEME_METRICS.iconSizeSmall;
  switch (action.type) {
    case "play":
    case "start": return <Play size={size} aria-hidden />;
    case "stop": return <Square size={size} aria-hidden />;
    case "toggle": return <Repeat size={size} aria-hidden />;
    case "mark": return <Flag size={size} aria-hidden />;
    case "set": return <SlidersHorizontal size={size} aria-hidden />;
  }
}

/** One action of a control: select it to edit it, or remove it from the control. */
function ActionChip({ document, entry, selected, setSelected }: {
  document: ConsoleGuiDocument;
  entry: Entry;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  const { on, index, action } = entry;
  const isSelected = selected?.type === "consoleAction" && selected.on === on.id && selected.index === index;
  const detail = actionDetail(action);
  return <span className={`action-chip${isSelected ? " selected" : ""}`}>
    <Button variant="ghost" className="action-chip-body" aria-pressed={isSelected} onClick={() => { setSelected({ type: "consoleAction", on: on.id, index }); }}>
      <ActionIcon action={action} />
      <span>{actionTarget(document, action)}</span>
      {detail !== null && <span className="action-chip-detail">{detail}</span>}
    </Button>
    <IconButton icon={X} label="Remove action" danger className="action-chip-remove" onClick={() => {
      void applyConsoleEdit({ type: "removeAction", on: on.id, index }).then(() => {
        if (isSelected) setSelected(null);
      });
    }} />
  </span>;
}

/** A cell's actions, and the menu that adds another. */
function ActionCell({ document, device, control, event, selected, setSelected }: {
  document: ConsoleGuiDocument;
  device: ConsoleInputDevice;
  control: ConsoleInputControl;
  event: ConsoleEventKind;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  const entries = document.events.flatMap((on): Entry[] => answers(on, event, device, control)
    ? on.actions.map((action, index) => ({ on, index, action })) : []);
  return <div className="control-cell actions">
    {entries.map((entry) => <ActionChip key={`${entry.on.id}:${entry.index}`} document={document} entry={entry} selected={selected} setSelected={setSelected} />)}
    <AddActionMenu document={document} device={device} control={control} event={event} setSelected={setSelected} />
  </div>;
}

const PLAY_MODES: { mode: ConsolePlayMode; label: string; releaseToo: boolean }[] = [
  { mode: "forTime", label: "Play for a moment", releaseToo: true },
  { mode: "held", label: "Play while held", releaseToo: false },
  { mode: "toggle", label: "Toggle", releaseToo: true }
];

/** The folders of definitions as nested submenus whose items pick a definition. */
function folderEntries(tree: DefinitionFolder<SequenceEffectDefinition>[], onSelect: (definition: SequenceEffectDefinition) => void): MenuEntry[] {
  return tree.map((folder) => ({
    type: "submenu",
    label: folder.name,
    entries: [
      ...folderEntries(folder.folders, onSelect),
      ...folder.items.map((definition): MenuEntry => ({ type: "action", label: definition.name, onSelect: () => { onSelect(definition); } }))
    ]
  }));
}

/** The + of an action cell: what the control can do on that event. */
function AddActionMenu({ document, device, control, event, setSelected }: {
  document: ConsoleGuiDocument;
  device: ConsoleInputDevice;
  control: ConsoleInputControl;
  event: ConsoleEventKind;
  setSelected: (focus: GuiFocus) => void;
}) {
  const tree = useMemo(() => definitionTree(document.effectDefinitions, (definition) => definition.effect.path), [document.effectDefinitions]);
  const target = document.targets[0];
  const ref = controlRef(device, control);
  const addEffect = (mode: ConsolePlayMode, definition: SequenceEffectDefinition) => {
    if (target === undefined) return;
    void applyConsoleEdit({ type: "addEffectAction", control: ref, event, mode, initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target })
      .then((update: GuiEditUpdate) => { setSelected(focusOnEffect(update, update.created[0])); });
  };
  const entries: MenuEntry[] = [
    ...PLAY_MODES.filter((choice) => event === "press" || choice.releaseToo).map((choice): MenuEntry => ({
      type: "submenu",
      label: choice.label,
      disabled: target === undefined || tree.length === 0,
      entries: folderEntries(tree, (definition) => { addEffect(choice.mode, definition); })
    })),
    {
      type: "submenu",
      label: "Stop",
      disabled: document.clips.length === 0,
      entries: document.clips.map((clip): MenuEntry => ({ type: "action", label: clip.name, onSelect: () => {
        void applyConsoleEdit({ type: "addAction", control: ref, event, action: { type: "stop", clipId: clip.id } });
      } }))
    },
    {
      type: "submenu",
      label: "Live mark",
      disabled: document.liveMarks.length === 0,
      entries: document.liveMarks.map((marks): MenuEntry => ({ type: "action", label: marks.key, onSelect: () => {
        void applyConsoleEdit({ type: "addAction", control: ref, event, action: { type: "mark", into: marks.key } });
      } }))
    }
  ];
  return <MenuButton entries={entries}><IconButton icon={Plus} label="Add action" className="action-add" /></MenuButton>;
}

/** Every button of the setup's input devices with what it does when pressed and released. Knobs are linked from the parameters they set. */
export function ConsoleControlGrid({ document, selected, setSelected }: {
  document: ConsoleGuiDocument;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  if (document.inputDevices.every((device) => device.buttons.length === 0)) {
    return <Badge tip="Add an input device with buttons to the project's setup to play this console">No buttons</Badge>;
  }
  return <div className="control-grid console">
    <div className="control-grid-row">
      <div className="control-cell head"><span>Control</span></div>
      <div className="control-cell head"><span>Input</span></div>
      <div className="control-cell head"><span>Press</span></div>
      <div className="control-cell head"><span>Release</span></div>
    </div>
    {document.inputDevices.filter((device) => device.buttons.length > 0).flatMap((device) => [
      <div className="control-grid-row" key={`${guiObjectKey(device.device)}:name`}>
        <div className="control-cell group"><span>{device.name}</span></div>
      </div>,
      ...device.buttons.map((control) => <div className="control-grid-row" key={`${guiObjectKey(device.device)}:${control.control}`}>
        <div className="control-cell name"><span>{control.name}</span></div>
        <div className="control-cell"><span>{inputLabel(control.input)}</span></div>
        <ActionCell document={document} device={device} control={control} event="press" selected={selected} setSelected={setSelected} />
        <ActionCell document={document} device={device} control={control} event="release" selected={selected} setSelected={setSelected} />
      </div>)
    ])}
  </div>;
}
