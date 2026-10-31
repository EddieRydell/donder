import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Flag, Play, Plus, Repeat, SlidersHorizontal, Square, X } from "lucide-react";
import { useContext, useMemo, type ReactNode } from "react";
import { THEME_COLORS, THEME_METRICS } from "../../../theme";
import type { ConsoleAction, ConsoleEventKind, ConsoleGuiDocument, ConsoleInputControl, ConsoleInputDevice, ConsoleOn, ConsolePlayMode, GuiEditUpdate, SequenceEffectDefinition } from "../../../types";
import { guiObjectKey } from "../../../workspace/guiIdentity";
import { OverlayPortal } from "../../OverlayPortal";
import { DefinitionMenuItems, definitionTree, effectReferenceKey } from "../sequence/definitionMenu";
import type { GuiFocus } from "../shared";
import { answers, applyConsoleEdit, controlRef, focusOnEffect, focusOnSet, freeParameters, inputLabel, targetLabel, type ParameterGroup } from "./consoleEditing";

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
    <button type="button" className="action-chip-body" aria-pressed={isSelected} onClick={() => { setSelected({ type: "consoleAction", on: on.id, index }); }}>
      <ActionIcon action={action} />
      <span>{actionTarget(document, action)}</span>
      {detail !== null && <span className="action-chip-detail">{detail}</span>}
    </button>
    <button type="button" className="action-chip-remove" aria-label="Remove action" title="Remove action" onClick={() => {
      void applyConsoleEdit({ type: "removeAction", on: on.id, index }).then(() => {
        if (isSelected) setSelected(null);
      });
    }}><X size={THEME_METRICS.iconSizeSmall} aria-hidden /></button>
  </span>;
}

/** A cell's actions, and the menu that adds another. */
function ActionCell({ document, device, control, event, selected, setSelected, className }: {
  document: ConsoleGuiDocument;
  device: ConsoleInputDevice;
  control: ConsoleInputControl;
  event: ConsoleEventKind;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
  className?: string;
}) {
  const entries = document.events.flatMap((on): Entry[] => answers(on, event, device, control)
    ? on.actions.map((action, index) => ({ on, index, action })) : []);
  return <div className={className === undefined ? "control-cell actions" : `control-cell actions ${className}`}>
    {entries.map((entry) => <ActionChip key={`${entry.on.id}:${entry.index}`} document={document} entry={entry} selected={selected} setSelected={setSelected} />)}
    <AddActionMenu document={document} device={device} control={control} event={event} setSelected={setSelected} />
  </div>;
}

const PLAY_MODES: { mode: ConsolePlayMode; label: string; releaseToo: boolean }[] = [
  { mode: "forTime", label: "Play an effect for a moment", releaseToo: true },
  { mode: "held", label: "Play an effect while held", releaseToo: false },
  { mode: "toggle", label: "Toggle an effect", releaseToo: true }
];

/** The + of an action cell: what the control can do on that event. */
function AddActionMenu({ document, device, control, event, setSelected }: {
  document: ConsoleGuiDocument;
  device: ConsoleInputDevice;
  control: ConsoleInputControl;
  event: ConsoleEventKind;
  setSelected: (focus: GuiFocus) => void;
}) {
  const container = useContext(OverlayPortal);
  const tree = useMemo(() => definitionTree(document.effectDefinitions, (definition) => definition.effect.path), [document.effectDefinitions]);
  const target = document.targets[0];
  const ref = controlRef(device, control);
  const addEffect = (mode: ConsolePlayMode, definition: SequenceEffectDefinition) => {
    if (target === undefined) return;
    void applyConsoleEdit({ type: "addEffectAction", control: ref, event, mode, initialColor: THEME_COLORS.white, effect: definition.effect, target: target.target })
      .then((update: GuiEditUpdate) => { setSelected(focusOnEffect(update, update.created[0])); });
  };
  const { Root, Trigger, Portal, Content, Item, Sub, SubTrigger, SubContent, Separator } = DropdownMenu;
  const submenu = (label: string, children: ReactNode, disabled = false) => <Sub>
    <SubTrigger className="menu-item" disabled={disabled}>{label}</SubTrigger>
    <Portal container={container}><SubContent className="menu-content">{children}</SubContent></Portal>
  </Sub>;
  const effects = (mode: ConsolePlayMode) => <DefinitionMenuItems menu={DropdownMenu} tree={tree} label={(definition) => definition.name}
    itemKey={(definition) => effectReferenceKey(definition.effect)} onSelect={(definition) => { addEffect(mode, definition); }} empty="No effects" disabled={target === undefined} />;
  const free = freeParameters(document);
  const link = (group: ParameterGroup) => submenu(group.label, group.params.map(({ name, target }) => <Item key={name} className="menu-item" onSelect={() => {
    void applyConsoleEdit({ type: "controlParam", target, control: ref }).then((update) => { setSelected(focusOnSet(update, target)); });
  }}>{name}</Item>));
  const setParameter = free.effects.length + free.operators.length === 0
    ? <Item className="menu-item" disabled>No parameters left to control</Item>
    : <>
      {free.effects.map(link)}
      {free.effects.length > 0 && free.operators.length > 0 && <Separator className="menu-separator" />}
      {free.operators.map(link)}
    </>;
  const axis = event === "move";
  return <Root>
    <Trigger className="action-add" aria-label="Add an action" title="Add an action"><Plus size={THEME_METRICS.iconSizeSmall} aria-hidden /></Trigger>
    <Portal container={container}>
      <Content className="menu-content" align="start" sideOffset={THEME_METRICS.menuOffset}>
        {!axis && PLAY_MODES.filter((choice) => event === "press" || choice.releaseToo).map((choice) => (
          <span key={choice.mode}>{submenu(choice.label, effects(choice.mode), target === undefined)}</span>
        ))}
        {!axis && submenu("Stop an effect", document.clips.length === 0
          ? <Item className="menu-item" disabled>No effects</Item>
          : document.clips.map((clip) => <Item key={clip.id} className="menu-item" onSelect={() => {
            void applyConsoleEdit({ type: "addAction", control: ref, event, action: { type: "stop", clipId: clip.id } });
          }}>{clip.name}</Item>))}
        {!axis && submenu("Add a live mark", document.liveMarks.length === 0
          ? <Item className="menu-item" disabled>No live mark collections</Item>
          : document.liveMarks.map((marks) => <Item key={marks.key} className="menu-item" onSelect={() => {
            void applyConsoleEdit({ type: "addAction", control: ref, event, action: { type: "mark", into: marks.key } });
          }}>{marks.key}</Item>))}
        {submenu(axis ? "Control a parameter" : "Link a parameter to velocity", setParameter)}
      </Content>
    </Portal>
  </Root>;
}

/** Every control of the setup's input devices with what it does: buttons when pressed and released, axes when moved. */
export function ConsoleControlGrid({ document, selected, setSelected }: {
  document: ConsoleGuiDocument;
  selected: GuiFocus;
  setSelected: (focus: GuiFocus) => void;
}) {
  if (document.inputDevices.every((device) => device.buttons.length === 0 && device.axes.length === 0)) {
    return <p className="object-overview-empty">Add an input device with buttons or axes to the project's setup to play this console.</p>;
  }
  const label = (control: ConsoleInputControl, axis: boolean) => <>
    <div className="control-cell name"><span>{control.name}{axis && <span className="control-kind"> axis</span>}</span></div>
    <div className="control-cell"><span>{inputLabel(control.input)}</span></div>
  </>;
  return <div className="control-grid console">
    <div className="control-grid-row">
      <div className="control-cell head"><span>Control</span></div>
      <div className="control-cell head"><span>Input</span></div>
      <div className="control-cell head"><span>When pressed</span></div>
      <div className="control-cell head"><span>When released</span></div>
    </div>
    {document.inputDevices.flatMap((device) => [
      <div className="control-grid-row" key={`${guiObjectKey(device.device)}:name`}>
        <div className="control-cell group"><span>{device.name}</span></div>
      </div>,
      ...device.buttons.map((control) => <div className="control-grid-row" key={`${guiObjectKey(device.device)}:${control.control}`}>
        {label(control, false)}
        <ActionCell document={document} device={device} control={control} event="press" selected={selected} setSelected={setSelected} />
        <ActionCell document={document} device={device} control={control} event="release" selected={selected} setSelected={setSelected} />
      </div>),
      ...device.axes.map((control) => <div className="control-grid-row" key={`${guiObjectKey(device.device)}:${control.control}`}>
        {label(control, true)}
        <ActionCell document={document} device={device} control={control} event="move" selected={selected} setSelected={setSelected} className="span" />
      </div>)
    ])}
  </div>;
}
