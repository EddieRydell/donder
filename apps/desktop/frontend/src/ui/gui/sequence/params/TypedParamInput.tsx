import { commands } from "../../../../api";
import { runGuiEditCommand } from "../../../../store";
import { navigateToGuiObject } from "../../../../workspace/navigation";
import { isSecondaryClick } from "../../../../platform";
import { matchesCommand } from "../../../../commandRegistry";
import { normalizeHexColor } from "../../../../color";
import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { ArrowDown, ArrowUp, FlipHorizontal2, FlipVertical2, Link2, Link2Off, Minus, Plus, SquareArrowOutUpRight, Trash2, X } from "lucide-react";

import { THEME_COLORS, THEME_METRICS } from "../../../../theme";

import type { SequenceGradientStop, SequenceCurvePoint, SequenceAutomationClip, SequenceAutomationTarget, SequenceParamRange, SequenceCurveLibraryItem, SequenceGradientLibraryItem, SequenceClipParam, SequenceParamValue, SequenceMarkCollection, SequenceCurveValue, SequenceGradientValue, SequenceGradientSpace, SequenceLibraryLink } from "../../../../types";

import { ColorPicker } from "../../../ColorPicker";
import { AddRow, ContextMenuArea, Field, IconButton, NumberField, Section, Select, Toggle, type MenuEntry } from "../../../kit";
import { automationTargetsEqual, clamp, type AutomationClipChooser } from "../../shared";
import { WindowFields } from "../../WindowFields";

/** The select value for a marks parameter with no collection; collection keys are never empty. */
const NO_MARK_COLLECTION = "";
/** The source select value for a value stored in the parameter itself. */
const CUSTOM_SOURCE = "custom";

const CURVE_EDITOR = {
  width: THEME_METRICS.curveEditorWidth,
  height: THEME_METRICS.curveEditorHeight,
  roundScale: 1000,
  flatRangeEpsilon: 0.0001,
  valueSteps: 20,
  colorMismatchDistance: 0.001,
  emptyGradient: THEME_COLORS.emptyGradient,
  defaultColor: THEME_COLORS.white
} as const;

export type EditedSequenceCurvePoint = { time: number; value: number };

export type EditedSequenceGradientStop = { time: number; value: string };

type ParamAutomationControls = {
  target: SequenceAutomationTarget;
  automationClips: SequenceAutomationClip[];
  canCreateAutomationClip: boolean;
  automationClipChooser: AutomationClipChooser;
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void;
};

export function TypedParamInput({
  param,
  commitParam,
  curveLibrary,
  gradientLibrary,
  markCollections,
  automation = null,
  automationControl = null
}: {
  param: SequenceClipParam;
  commitParam: (name: string, value: SequenceParamValue) => Promise<void>;
  curveLibrary: SequenceCurveLibraryItem[];
  gradientLibrary: SequenceGradientLibraryItem[];
  markCollections: Pick<SequenceMarkCollection, "key">[];
  automation?: ParamAutomationControls | null;
  /** Controls that link the parameter to its automation, in place of a sequence's. */
  automationControl?: ReactNode;
}) {

  const commit = (value: SequenceParamValue) => {
    return commitParam(param.name, value);
  };
  const automated = param.automation !== null;
  const tip = paramTip(param);

  if (!param.editable && !automated) {
    return <Field label={param.name} tip={tip}><span className="effect-param-unavailable">Unavailable</span></Field>;
  }

  const automationActions = automationControl !== null ? automationControl :
    automation === null || !param.supportsAutomation
      ? null
      : automationBindingControl(
          automation.target,
          param,
          automation.automationClips,
          automation.canCreateAutomationClip,
          automation.automationClipChooser,
          automation.setAutomationClipChooser
        );
  // An automated parameter shows no value of its own: automation sets it, so only its controls remain.
  const valueField = (control: ReactNode) => (
    <Field label={param.name} tip={tip}>
      {automated ? null : control}
      {automationActions}
    </Field>
  );
  switch (param.value.type) {
    case "int":
      return valueField(<NumberParam name={param.name} value={param.value.value} step={1} integer range={param.range} commit={(value) => commit({ type: "int", value })} />);
    case "float":
      return valueField(<NumberParam name={param.name} value={param.value.value} step={0.05} integer={false} range={param.range} commit={(value) => commit({ type: "float", value })} />);
    case "bool":
      return valueField(<Toggle name={param.name} checked={param.value.value} onChange={(value) => void commit({ type: "bool", value })} />);
    case "color":
      return (
        <Field label={param.name} tip={tip}>
          <ColorPicker
            value={param.value.value}
            label={param.name}
            commit={(value) => commit({ type: "color", value })}
          />
        </Field>
      );
    case "enum":
      return valueField(
        <Select
          name={param.name}
          value={param.value.value}
          options={param.options.map((option) => ({ value: option, label: option }))}
          onChange={(value) => void commit({ type: "enum", value })}
        />
      );
    case "curve":
      return (
        <CurveValueEditor name={param.name} tip={tip} value={param.value.value} declarations={curveLibrary} range={param.range}
          commit={(value) => commit({ type: "curve", value })}
          disabled={automated} actions={automationActions} />
      );
    case "gradient":
      return (
        <GradientValueEditor name={param.name} tip={tip} value={param.value.value} declarations={gradientLibrary}
          commit={(value) => commit({ type: "gradient", value })} />
      );
    case "intArray":
      return <NumberArrayParam name={param.name} tip={tip} values={param.value.values} step={1} integer commit={(values) => commit({ type: "intArray", values })} />;
    case "floatArray":
      return <NumberArrayParam name={param.name} tip={tip} values={param.value.values} step={0.05} integer={false} commit={(values) => commit({ type: "floatArray", values })} />;
    case "boolArray":
      return <BoolArrayParam name={param.name} tip={tip} values={param.value.values} commit={(values) => commit({ type: "boolArray", values })} />;
    case "colorArray":
      return <ColorArrayParam name={param.name} tip={tip} values={param.value.values} commit={(values) => commit({ type: "colorArray", values })} />;
    case "curveArray":
      return <CurveArrayParam declarations={curveLibrary} name={param.name} tip={tip} values={param.value.values} range={param.range} commit={(values) => commit({ type: "curveArray", values })} />;
    case "gradientArray":
      return <GradientArrayParam declarations={gradientLibrary} name={param.name} tip={tip} values={param.value.values} commit={(values) => commit({ type: "gradientArray", values })} />;
    case "marks":
      return (
        <Field label={param.name} tip={tip}>
          <Select
            name={param.name}
            value={param.value.key ?? NO_MARK_COLLECTION}
            options={[
              { value: NO_MARK_COLLECTION, label: "None" },
              ...markCollections.map((collection) => ({ value: collection.key, label: collection.key }))
            ]}
            onChange={(key) => void commit({ type: "marks", key: key === NO_MARK_COLLECTION ? null : key })}
          />
        </Field>
      );
  }
}

function formatRange(range: SequenceParamRange) {
  return `${roundCurveValue(range.min)}..${roundCurveValue(range.max)}`;
}

/** The hover text of a param's name: its description and declared range. */
function paramTip(param: SequenceClipParam): string | undefined {
  const parts = [param.description, param.range === null ? null : formatRange(param.range)].filter((part) => part !== null);
  return parts.length === 0 ? undefined : parts.join(" · ");
}

function NumberParam({ name, value, step, integer, range, commit }: {
  name: string;
  value: number;
  step: number;
  integer: boolean;
  range: SequenceParamRange | null;
  commit: (value: number) => Promise<void>;
}) {
  return <NumberField
    name={name}
    value={value}
    step={step}
    integer={integer}
    {...(range === null ? {} : { min: range.min, max: range.max })}
    onCommit={(next) => void commit(next)}
  />;
}

function NumberArrayParam({ name, tip, values, step, integer, commit }: { name: string; tip: string | undefined; values: number[]; step: number; integer: boolean; commit: (values: number[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      tip={tip}
      values={values}
      newValue={() => values[values.length - 1] ?? 0}
      commit={commit}
      render={(value, index) => (
        <NumberField name={`${name} ${index + 1}`} value={value} step={step} integer={integer} onCommit={(next) => void commit(replaceAt(values, index, next))} />
      )}
    />
  );
}

function automationBindingControl(target: SequenceAutomationTarget,
  param: SequenceClipParam,
  clips: SequenceAutomationClip[],
  canCreateAutomationClip: boolean,
  automationClipChooser: AutomationClipChooser,
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void
) {

  const choosing = automationClipChooser !== null && automationTargetsEqual(automationClipChooser.target, target);
  if (param.automation !== null) {
    const { automationClipId, window: bounds } = param.automation;
    return (
      <>
        <WindowFields
          value={bounds}
          range={param.range}
          onChange={(next) =>
            void runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "setAutomationWindow",
                automationClipId,
                target,
                window: next
              } })
            )
          }
        />
        <IconButton
          icon={Link2Off}
          label="Unlink automation"
          onClick={() =>
            void runGuiEditCommand((request) =>
              commands.applyGuiEdit(request, { type: "sequence", edit: {
                type: "unbindAutomationParam",
                automationClipId,
                target
              } })
            )
          }
        />
      </>
    );
  }
  return (
    <>
      <IconButton
        icon={Link2}
        label="Link existing automation"
        disabled={clips.length === 0}
        onClick={() => {
          setAutomationClipChooser({ target });
        }}
      />
      <IconButton
        icon={Plus}
        label="Create automation"
        disabled={!canCreateAutomationClip}
        onClick={() =>
          void runGuiEditCommand((request) =>
            commands.applyGuiEdit(request, { type: "sequence", edit: {
              type: "createAndBindAutomationClip",
              target
            } })
          )
        }
      />
      {choosing && <IconButton icon={X} label="Cancel choosing" onClick={() => { setAutomationClipChooser(null); }} />}
    </>
  );
}

function BoolArrayParam({ name, tip, values, commit }: { name: string; tip: string | undefined; values: boolean[]; commit: (values: boolean[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      tip={tip}
      values={values}
      newValue={() => false}
      commit={commit}
      render={(value, index) => (
        <Toggle name={`${name} ${index + 1}`} checked={value} onChange={(checked) => void commit(replaceAt(values, index, checked))} />
      )}
    />
  );
}

function ColorArrayParam({ name, tip, values, commit }: { name: string; tip: string | undefined; values: string[]; commit: (values: string[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      tip={tip}
      values={values}
      newValue={() => values[values.length - 1] ?? CURVE_EDITOR.defaultColor}
      commit={commit}
      render={(value, index) => (
        <ColorPicker label={`${name} ${index + 1}`} value={value} commit={(next) => commit(replaceAt(values, index, next))} />
      )}
    />
  );
}

function CurveArrayParam({ name, tip, values, declarations, range, commit }: { name: string; tip: string | undefined; values: SequenceCurveValue[]; declarations: SequenceCurveLibraryItem[]; range: SequenceParamRange | null; commit: (values: SequenceCurveValue[]) => Promise<void> }) {
  const newValue = (): SequenceCurveValue => {
    const axis = curveAxis([], range);
    return values[values.length - 1] ?? { points: [{ time: 0, value: axis.min }, { time: 1, value: axis.max }], link: { type: "inline" } };
  };
  return <ArrayShell name={name} tip={tip} values={values}
    newValue={newValue}
    commit={commit}
    render={(value, index) => <CurveValueEditor name={`#${index + 1}`} tip={undefined} value={value} declarations={declarations} range={range} commit={(next) => commit(replaceAt(values, index, next))} />} />;
}

function GradientArrayParam({ name, tip, values, declarations, commit }: { name: string; tip: string | undefined; values: SequenceGradientValue[]; declarations: SequenceGradientLibraryItem[]; commit: (values: SequenceGradientValue[]) => Promise<void> }) {
  return <ArrayShell name={name} tip={tip} values={values}
    newValue={(): SequenceGradientValue => values[values.length - 1] ?? { gradient: { space: "linearRgb", stops: [{ time: 0, value: CURVE_EDITOR.defaultColor }] }, link: { type: "inline" } }}
    commit={commit}
    render={(value, index) => <GradientValueEditor name={`#${index + 1}`} tip={undefined} value={value} declarations={declarations} commit={(next) => commit(replaceAt(values, index, next))} />} />;
}

function ArrayShell<T>({
  name,
  tip,
  values,
  newValue,
  commit,
  render
}: {
  name: string;
  tip: string | undefined;
  values: T[];
  newValue: () => T;
  commit: (values: T[]) => Promise<void>;
  render: (value: T, index: number) => ReactNode;
}) {
  const add = () => void commit([...values, newValue()]);
  return (
    <div className="effect-param-group array-param-editor">
      <Field label={name} tip={tip}>
        {values.length === 0 ? <AddRow label="Add item" onAdd={add} /> : <IconButton icon={Plus} label="Add item" onClick={add} />}
      </Field>
      {values.map((value, index) => (
        <div key={index} className="array-param-row">
          <div className="array-param-row-main">{render(value, index)}</div>
          <div className="array-param-actions">
            <IconButton icon={ArrowUp} label="Move up" disabled={index === 0} onClick={() => void commit(moveArrayItem(values, index, index - 1))} />
            <IconButton icon={ArrowDown} label="Move down" disabled={index >= values.length - 1} onClick={() => void commit(moveArrayItem(values, index, index + 1))} />
            <IconButton icon={Trash2} label="Remove item" danger onClick={() => void commit(values.filter((_, itemIndex) => itemIndex !== index))} />
          </div>
        </div>
      ))}
    </div>
  );
}

type CurveEditorProps<T extends { time: number }> = {
  points: T[];
  commit: (points: T[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
  showName?: boolean;
};

function CurveValueEditor({ name, tip, value, declarations, range, commit, disabled = false, actions = null }: {
  name: string;
  tip: string | undefined;
  value: SequenceCurveValue;
  declarations: SequenceCurveLibraryItem[];
  range: SequenceParamRange | null;
  commit: (value: SequenceCurveValue) => Promise<void>;
  disabled?: boolean;
  actions?: ReactNode;
}) {
  const points = normalizeSequenceCurvePoints(value.points);
  return <LibraryValueShell name={name} tip={tip} label="curve" libraryLink={value.link} declarations={declarations}
    points={points} commit={(points) => commit({ points, link: { type: "inline" } })}
    disabled={disabled} actions={actions}
    link={(curve) => commit({ points: curve.points, link: { type: "library", path: curve.path, objectKey: curve.objectKey, displayName: curve.displayName } })}
    unlink={() => commit({ points, link: { type: "inline" } })}
    flipVerticalPoints={(current) => {
      const axis = curveAxis(current, range);
      return current.map((point) => ({ ...point, value: roundCurveValue(axis.min + axis.max - point.value) }));
    }}
    render={(props) => <CurveParam name={name} range={range} {...props} />} />;
}

function GradientValueEditor({ name, tip, value, declarations, commit }: {
  name: string;
  tip: string | undefined;
  value: SequenceGradientValue;
  declarations: SequenceGradientLibraryItem[];
  commit: (value: SequenceGradientValue) => Promise<void>;
}) {
  const { space } = value.gradient;
  const points = normalizeSequenceGradientStops(value.gradient.stops);
  return <LibraryValueShell name={name} tip={tip} label="gradient" libraryLink={value.link} declarations={declarations}
    points={points} commit={(stops) => commit({ gradient: { space, stops }, link: { type: "inline" } })}
    link={(item) => commit({ gradient: item.gradient, link: { type: "library", path: item.path, objectKey: item.objectKey, displayName: item.displayName } })}
    unlink={() => commit({ gradient: { space, stops: points }, link: { type: "inline" } })}
    render={(props) => <GradientParam name={name} space={space} commitSpace={(next) => commit({ gradient: { space: next, stops: points }, link: { type: "inline" } })} {...props} />} />;
}

type LibraryItem = { path: string; objectKey: string; displayName: string };

/**
 * A curve or gradient that is either stored in the parameter or linked to a library declaration.
 * A linked value is read-only; editing or flipping it makes it independent, which undo reverts.
 */
function LibraryValueShell<T extends { time: number }, S extends LibraryItem>({
  name,
  tip,
  label,
  libraryLink,
  declarations,
  points,
  commit,
  disabled = false,
  actions = null,
  link,
  unlink,
  flipVerticalPoints,
  render
}: {
  name: string;
  tip: string | undefined;
  label: "curve" | "gradient";
  libraryLink: SequenceLibraryLink;
  declarations: S[];
  points: T[];
  commit: (points: T[]) => Promise<void>;
  disabled?: boolean;
  actions?: ReactNode;
  link: (declaration: S) => Promise<void>;
  unlink: () => Promise<void>;
  flipVerticalPoints?: (points: T[]) => T[];
  render: (props: CurveEditorProps<T>) => ReactNode;
}) {

  const linkedDeclaration = libraryLink.type === "library" ? libraryLink : null;
  const selectedDeclarationIndex = linkedDeclaration === null
    ? -1
    : declarations.findIndex((item) => item.path === linkedDeclaration.path && item.objectKey === linkedDeclaration.objectKey);
  const sourceOptions = [
    { value: CUSTOM_SOURCE, label: "Custom" },
    ...(linkedDeclaration !== null && selectedDeclarationIndex === -1 ? [{ value: "library:-1", label: linkedDeclaration.displayName }] : []),
    ...declarations.map((item, index) => ({ value: `library:${index}`, label: item.displayName }))
  ];
  return (
    <div className={`effect-param-group param-source-shell ${linkedDeclaration !== null ? "linked" : ""}`}>
      <Field label={name} tip={tip}>
        <Select
          name={`${name} ${label}`}
          disabled={disabled}
          value={linkedDeclaration === null ? CUSTOM_SOURCE : `library:${selectedDeclarationIndex}`}
          options={sourceOptions}
          onChange={(value) => {
            if (value === CUSTOM_SOURCE) {
              if (linkedDeclaration !== null) void unlink();
              return;
            }
            const declaration = declarations[Number(value.replace("library:", ""))];
            if (declaration !== undefined) void link(declaration);
          }}
        />
        {linkedDeclaration !== null && <>
          <IconButton
            icon={Link2}
            pressed
            disabled={disabled}
            label={`Linked to ${linkedDeclaration.displayName} in ${linkedDeclaration.path}. Make independent`}
            onClick={() => void unlink()}
          />
          <IconButton
            icon={SquareArrowOutUpRight}
            label={`Open ${linkedDeclaration.displayName}`}
            onClick={() => void navigateToGuiObject({ ownedPath: [], path: linkedDeclaration.path, objectKey: linkedDeclaration.objectKey })}
          />
        </>}
      </Field>
      {(!disabled || actions !== null) && (
        <div className="param-source-actions">
          {!disabled && <>
            <IconButton
              icon={FlipHorizontal2}
              label="Flip horizontal"
              onClick={() => void commit(sortCurvePoints(points.map((point) => ({ ...point, time: roundCurveValue(1 - point.time) }))))}
            />
            {flipVerticalPoints !== undefined && <IconButton icon={FlipVertical2} label="Flip vertical" onClick={() => void commit(flipVerticalPoints(points))} />}
          </>}
          {actions}
        </div>
      )}
      {render({
        points,
        commit,
        readOnly: disabled || linkedDeclaration !== null,
        ...(disabled ? {} : { requestInlineEdit: () => void unlink() }),
        showName: false
      })}
    </div>
  );
}

export function CurveParam({
  name,
  points,
  range = null,
  commit,
  readOnly = false,
  requestInlineEdit,
  showName = true
}: {
  name: string;
  points: EditedSequenceCurvePoint[];
  /** The param's declared value range fixes the axis; without one it fits the points. */
  range?: SequenceParamRange | null;
  commit: (points: EditedSequenceCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
  showName?: boolean;
}) {
  const { drafts, draftsRef, selectedIndex, setSelectedIndex, update, setPoint, deletePoint } = usePointDrafts({
    points, commit, readOnly, requestInlineEdit, normalize: normalizeCurvePoint, nearest: nearestFloatPointIndex
  });
  const svgRef = useRef<SVGSVGElement | null>(null);
  const draggingPoint = useRef<number | null>(null);
  const valueRange = curveAxis(drafts, range);
  const path = curveSvgPath(drafts, valueRange);
  const pointFromPointer = (event: PointerEvent<SVGSVGElement>): EditedSequenceCurvePoint => {
    const rect = svgRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: 0 };
    const x = clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1);
    const y = clamp((event.clientY - rect.top) / Math.max(1, rect.height), 0, 1);
    return {
      time: roundCurveValue(x),
      value: roundCurveValue(valueRange.max - y * (valueRange.max - valueRange.min))
    };
  };
  return (
    <div className="effect-param-group float-curve-editor">
      {showName && <div className="effect-param-name">{name}</div>}
      <svg
        ref={svgRef}
        className="float-curve-graph"
        viewBox={`0 0 ${CURVE_EDITOR.width} ${CURVE_EDITOR.height}`}
        role="img"
        aria-label={`${name} curve`}
        onPointerDown={(event) => {
          if (isSecondaryClick(event)) return;
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          if (event.target instanceof SVGCircleElement) return;
          const point = pointFromPointer(event);
          update([...draftsRef.current, point]);
          setSelectedIndex(nearestFloatPointIndex(draftsRef.current, point));
        }}
        onPointerMove={(event) => {
          const index = draggingPoint.current;
          if (index === null) return;
          draggingPoint.current = setPoint(index, pointFromPointer(event), false);
        }}
        onPointerUp={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
        onPointerCancel={(event) => {
          if (draggingPoint.current === null) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          draggingPoint.current = null;
          update(draftsRef.current);
        }}
      >
        <rect className="float-curve-graph-bg" x="0" y="0" width={CURVE_EDITOR.width} height={CURVE_EDITOR.height} />
        <path className="float-curve-grid-line" d={`M0 ${CURVE_EDITOR.height / 2}H${CURVE_EDITOR.width}`} />
        <path className="float-curve-grid-line" d={`M${CURVE_EDITOR.width / 2} 0V${CURVE_EDITOR.height}`} />
        <path className="float-curve-line" d={path} />
        {!readOnly && drafts.map((point, index) => {
          const x = point.time * CURVE_EDITOR.width;
          const y = CURVE_EDITOR.height - ((point.value - valueRange.min) / (valueRange.max - valueRange.min)) * CURVE_EDITOR.height;
          return (
            <ContextMenuArea key={index} entries={pointMenu(drafts.length, () => { deletePoint(index); })}>
              <circle
                className={`float-curve-point ${index === selectedIndex ? "selected" : ""}`}
                cx={x}
                cy={y}
                r={index === selectedIndex ? THEME_METRICS.curvePointRadiusSelected : THEME_METRICS.curvePointRadius}
                tabIndex={0}
                onPointerDown={(event) => {
                  event.stopPropagation();
                  if (isSecondaryClick(event)) return;
                  event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId);
                  draggingPoint.current = index;
                  setSelectedIndex(index);
                }}
                onContextMenu={() => { setSelectedIndex(index); }}
                onKeyDown={(event) => { deletePointOnKey(event, () => { deletePoint(index); }); }}
                onFocus={() => { setSelectedIndex(index); }}
              />
            </ContextMenuArea>
          );
        })}
      </svg>
      {!readOnly && (
        <Section title={`Points · ${drafts.length}`} defaultOpen={false}>
          <div className="float-curve-point-list">
            {drafts.map((point, index) => (
              <div
                key={index}
                className={`float-curve-point-row ${index === selectedIndex ? "selected" : ""}`}
                onPointerDown={() => { setSelectedIndex(index); }}
              >
                <NumberField name={`Point ${index + 1} time`} label="t" value={point.time} min={0} max={1} step={0.01}
                  onCommit={(time) => { setPoint(index, { ...point, time }, true); }} />
                <NumberField name={`Point ${index + 1} value`} label="v" value={point.value} step={curveValueStep(valueRange)}
                  {...(range === null ? {} : { min: range.min, max: range.max })}
                  onCommit={(value) => { setPoint(index, { ...point, value }, true); }} />
                <IconButton icon={Minus} label="Delete point" disabled={drafts.length <= 1} onClick={() => { deletePoint(index); }} />
              </div>
            ))}
          </div>
        </Section>
      )}
    </div>
  );
}

export function GradientParam({
  name,
  space,
  points,
  commit,
  commitSpace,
  readOnly = false,
  requestInlineEdit,
  showName = true
}: {
  name: string;
  space: SequenceGradientSpace;
  points: EditedSequenceGradientStop[];
  commit: (points: EditedSequenceGradientStop[]) => Promise<void>;
  commitSpace: (space: SequenceGradientSpace) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
  showName?: boolean;
}) {
  const { drafts, draftsRef, update, setPoint, deletePoint } = usePointDrafts({
    points, commit, readOnly, requestInlineEdit, normalize: normalizeGradientStop, nearest: nearestColorPointIndex
  });
  const [stopPickerRequest, setStopPickerRequest] = useState<{ index: number; key: number } | null>(null);
  const stopPickerRequestKey = useRef(0);
  const gradientRef = useRef<HTMLDivElement | null>(null);
  const draggingPoint = useRef<{ index: number; pointerId: number; startClientX: number; moved: boolean } | null>(null);
  const pointFromClientX = (clientX: number, color: string): EditedSequenceGradientStop => {
    const rect = gradientRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: color };
    return {
      time: roundCurveValue(clamp((clientX - rect.left) / Math.max(1, rect.width), 0, 1)),
      value: color
    };
  };
  const endStopDrag = (pointerId: number, cancelled: boolean) => {
    const drag = draggingPoint.current;
    if (drag === null || drag.pointerId !== pointerId) return;
    draggingPoint.current = null;
    if (drag.moved) {
      update(draftsRef.current);
    } else if (!cancelled) {
      setStopPickerRequest((request) =>
        request?.index === drag.index
          ? null
          : { index: drag.index, key: ++stopPickerRequestKey.current }
      );
    }
  };
  const moveStopDrag = (pointerId: number, clientX: number) => {
    const drag = draggingPoint.current;
    if (drag === null || drag.pointerId !== pointerId) return;
    const point = draftsRef.current[drag.index];
    if (point === undefined) return;
    const moved = drag.moved || Math.abs(clientX - drag.startClientX) > THEME_METRICS.interactionDragThreshold;
    if (!moved) return;
    const nextIndex = setPoint(drag.index, pointFromClientX(clientX, point.value), false);
    draggingPoint.current = { ...drag, index: nextIndex, moved: true };
  };
  const startStopDrag = (index: number, pointerId: number, clientX: number) => {
    draggingPoint.current = { index, pointerId, startClientX: clientX, moved: false };
    const handlePointerMove = (event: globalThis.PointerEvent) => {
      moveStopDrag(event.pointerId, event.clientX);
    };
    const handlePointerUp = (event: globalThis.PointerEvent) => {
      cleanup();
      endStopDrag(event.pointerId, false);
    };
    const handlePointerCancel = (event: globalThis.PointerEvent) => {
      cleanup();
      endStopDrag(event.pointerId, true);
    };
    const cleanup = () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerCancel);
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerCancel);
  };
  const gradient = gradientCss(drafts, space);
  return (
    <div className="effect-param-group color-curve-editor">
      {showName && <div className="effect-param-name">{name}</div>}
      <Select
        name={`${name} interpolation`}
        value={space}
        options={GRADIENT_SPACES}
        onChange={(next) => {
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          void commitSpace(next);
        }}
      />
      <div
        ref={gradientRef}
        className="color-curve-gradient"
        style={{ background: gradient }}
        onPointerDown={(event) => {
          if (isSecondaryClick(event)) return;
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          if (event.target !== event.currentTarget) return;
          const previous = draftsRef.current[draftsRef.current.length - 1]?.value ?? CURVE_EDITOR.defaultColor;
          update([...draftsRef.current, pointFromClientX(event.clientX, previous)]);
        }}
      >
        {!readOnly && drafts.map((point, index) => {
          return (
            <ContextMenuArea key={index} entries={pointMenu(drafts.length, () => { deletePoint(index); })}>
              <span
                className="color-curve-stop"
                draggable={false}
                style={{ left: `${point.time * 100}%` }}
                onPointerDown={(event) => {
                  if (!event.currentTarget.contains(event.target as Node)) return;
                  if (event.button !== 0 || isSecondaryClick(event)) return;
                  event.preventDefault();
                  event.stopPropagation();
                  startStopDrag(index, event.pointerId, event.clientX);
                }}
                onDragStart={(event) => {
                  event.preventDefault();
                }}
                onKeyDown={(event) => {
                  // Keys typed in the stop's color popover bubble here through the portal.
                  if (event.currentTarget.contains(event.target as Node)) deletePointOnKey(event, () => { deletePoint(index); });
                }}
              >
                <span className="color-curve-stop-line" />
                <ColorPicker
                  className="color-curve-stop-color-picker"
                  triggerClassName="color-curve-stop-color-trigger"
                  openRequestKey={stopPickerRequest?.index === index ? stopPickerRequest.key : 0}
                  onOpenChange={(open) => {
                    if (!open && stopPickerRequest?.index === index) setStopPickerRequest(null);
                  }}
                  value={normalizeHexColor(point.value) ?? points[index]?.value ?? CURVE_EDITOR.defaultColor}
                  label={`${name} stop ${index + 1} color`}
                  commit={(value) => {
                    setPoint(index, { ...point, value }, true);
                    return Promise.resolve();
                  }}
                />
              </span>
            </ContextMenuArea>
          );
        })}
      </div>
    </div>
  );
}

/**
 * The editable copy of a curve's points or a gradient's stops. Edits show at once and commit when
 * they change the value; a new value from the project replaces the copy unless it is the commit
 * still in flight.
 */
function usePointDrafts<T extends { time: number; value: number | string }>({ points, commit, readOnly, requestInlineEdit, normalize, nearest }: {
  points: T[];
  commit: (points: T[]) => Promise<void>;
  readOnly: boolean;
  requestInlineEdit: (() => void) | undefined;
  /** The point as it is committed, or `null` when it is invalid. */
  normalize: (point: T) => T | null;
  /** The index in `points` of the point closest to `point`. */
  nearest: (points: T[], point: T) => number;
}) {
  const [drafts, setDrafts] = useState(points);
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const pointsSignature = curvePointsSignature(points);
  const lastCommittedSignature = useRef(pointsSignature);
  const pendingSignature = useRef<string | null>(null);
  useEffect(() => {
    draftsRef.current = drafts;
  }, [drafts]);
  useEffect(() => {
    const draftSignature = curvePointsSignature(draftsRef.current);
    if (pointsSignature === draftSignature) {
      pendingSignature.current = null;
      return;
    }
    if (pendingSignature.current === draftSignature) return;
    setDrafts(points);
    draftsRef.current = points;
    lastCommittedSignature.current = pointsSignature;
    setSelectedIndex((index) => Math.min(index, points.length - 1));
  }, [points, pointsSignature]);
  const show = (next: T[]) => {
    setDrafts(next);
    draftsRef.current = next;
  };
  const update = (next: T[]) => {
    if (readOnly) {
      requestInlineEdit?.();
      return;
    }
    const normalized = next.map(normalize).filter((point): point is T => point !== null);
    if (normalized.length === 0 || normalized.length !== next.length) return;
    const sorted = sortCurvePoints(normalized);
    const signature = curvePointsSignature(sorted);
    show(sorted);
    setSelectedIndex((index) => Math.min(index, sorted.length - 1));
    if (signature === lastCommittedSignature.current) return;
    lastCommittedSignature.current = signature;
    pendingSignature.current = signature;
    void commit(sorted).catch(() => {
      pendingSignature.current = null;
    });
  };
  const setPoint = (index: number, point: T, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearest(next, point);
    show(next);
    setSelectedIndex(nextIndex);
    if (commitChange) update(next);
    return nextIndex;
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  return { drafts, draftsRef, selectedIndex, setSelectedIndex, update, setPoint, deletePoint };
}

/** A curve point's or gradient stop's right-click menu; the last one cannot be deleted. */
function pointMenu(count: number, onDelete: () => void): MenuEntry[] {
  return [{ type: "action", label: "Delete", icon: Trash2, danger: true, shortcut: "edit.delete", disabled: count <= 1, onSelect: onDelete }];
}

function deletePointOnKey(event: KeyboardEvent, onDelete: () => void) {
  if (!matchesCommand(event.nativeEvent, "edit.delete")) return;
  event.preventDefault();
  event.stopPropagation();
  onDelete();
}

function normalizeCurvePoint(point: EditedSequenceCurvePoint): EditedSequenceCurvePoint | null {
  return Number.isFinite(point.time) && Number.isFinite(point.value) ? point : null;
}

function normalizeGradientStop(point: EditedSequenceGradientStop): EditedSequenceGradientStop | null {
  const value = normalizeHexColor(point.value);
  return Number.isFinite(point.time) && value !== null ? { ...point, value } : null;
}

function replaceAt<T>(items: T[], index: number, value: T) {
  return items.map((item, itemIndex) => (itemIndex === index ? value : item));
}

function moveArrayItem<T>(items: T[], from: number, to: number) {
  if (to < 0 || to >= items.length) return items;
  const next = [...items];
  const [item] = next.splice(from, 1);
  if (item === undefined) return items;
  next.splice(to, 0, item);
  return next;
}

function sortCurvePoints<T extends { time: number }>(points: T[]) {
  return [...points].sort((left, right) => left.time - right.time);
}

function curveAxis(points: EditedSequenceCurvePoint[], range: SequenceParamRange | null) {
  if (range !== null && range.max > range.min) return range;
  const values = points.map((point) => point.value).filter(Number.isFinite);
  const min = Math.min(0, ...values);
  const max = Math.max(1, ...values);
  if (Math.abs(max - min) < CURVE_EDITOR.flatRangeEpsilon) return { min: min - 0.5, max: max + 0.5 };
  return { min, max };
}

/** One twentieth of the axis, as 0.05 is of a unit axis. */
function curveValueStep(axis: { min: number; max: number }) {
  return roundCurveValue((axis.max - axis.min) / CURVE_EDITOR.valueSteps);
}

function curveSvgPath(points: EditedSequenceCurvePoint[], range: { min: number; max: number }) {
  const sorted = sortCurvePoints(points);
  if (sorted.length === 0) return "";
  return sorted
    .map((point, index) => {
      const x = point.time * CURVE_EDITOR.width;
      const y = CURVE_EDITOR.height - ((point.value - range.min) / (range.max - range.min)) * CURVE_EDITOR.height;
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

function nearestFloatPointIndex(points: EditedSequenceCurvePoint[], point: EditedSequenceCurvePoint) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + Math.abs(candidate.value - point.value);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function nearestColorPointIndex(points: EditedSequenceGradientStop[], point: EditedSequenceGradientStop) {
  let bestIndex = 0;
  let bestDistance = Infinity;
  points.forEach((candidate, index) => {
    const distance = Math.abs(candidate.time - point.time) + (candidate.value === point.value ? 0 : CURVE_EDITOR.colorMismatchDistance);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  });
  return bestIndex;
}

/** The gradient spaces, each with the CSS color space its editor strip approximates it in. */
const GRADIENT_SPACES: { value: SequenceGradientSpace; label: string; css: string }[] = [
  { value: "linearRgb", label: "Linear RGB", css: "" },
  { value: "hsv", label: "HSV", css: "in hsl " },
  { value: "oklab", label: "Oklab", css: "in oklab " }
];

function gradientCss(points: EditedSequenceGradientStop[], space: SequenceGradientSpace) {
  const stops = sortCurvePoints(points)
    .flatMap((point) => {
      const color = normalizeHexColor(point.value);
      return color === null ? [] : [`${color} ${clamp(point.time, 0, 1) * 100}%`];
    });
  if (stops.length === 0) return CURVE_EDITOR.emptyGradient;
  if (stops.length === 1) return stops[0]?.split(" ")[0] ?? CURVE_EDITOR.emptyGradient;
  return `linear-gradient(${GRADIENT_SPACES.find((option) => option.value === space)?.css ?? ""}90deg, ${stops.join(", ")})`;
}

function roundCurveValue(value: number) {
  return Math.round(value * CURVE_EDITOR.roundScale) / CURVE_EDITOR.roundScale;
}

function curvePointsSignature(points: Array<{ time: number; value: number | string }>) {
  return JSON.stringify(points);
}

function normalizeSequenceCurvePoints(points: SequenceCurvePoint[]): EditedSequenceCurvePoint[] {
  const normalized = points
    .filter((point) => Number.isFinite(point.time) && Number.isFinite(point.value))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: 0 }];
}

function normalizeSequenceGradientStops(points: SequenceGradientStop[]): EditedSequenceGradientStop[] {
  const normalized = points
    .flatMap((point) => normalizeGradientStop(point) ?? [])
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: CURVE_EDITOR.defaultColor }];
}

