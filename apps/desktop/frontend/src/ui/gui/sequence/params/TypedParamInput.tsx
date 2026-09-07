import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from "react";
import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { ArrowDown, ArrowUp, ChevronRight, CopyPlus, FlipHorizontal2, FlipVertical2, Link2, Link2Off, Minus, Plus, Trash2, X } from "lucide-react";

import { commands } from "../../../../api";
import { THEME_COLORS, THEME_METRICS } from "../../../../theme";

import type { SequenceGradientStop, SequenceCurvePoint, SequenceAutomationClip, SequenceAutomationMapping, SequenceAutomationTarget, SequenceCurveLibraryItem, SequenceGradientLibraryItem, SequenceEffectParam, SequenceEffectParamValue, SequenceMarkCollection, SequenceCurveSource, SequenceGradientSource } from "../../../../types";

import { runGuiEditCommand } from "../../../../store";

import { ColorPicker } from "../../../ColorPicker";
import { automationTargetsEqual, clamp, type AutomationClipChooser } from "../../shared";

const CURVE_EDITOR = {
  width: THEME_METRICS.curveEditorWidth,
  height: THEME_METRICS.curveEditorHeight,
  roundScale: 1000,
  flatRangeEpsilon: 0.0001,
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

export function TypedParamInput(props: Parameters<typeof TypedParamValue>[0]) {
  return <>
    {props.param.fixed && <div className="effect-param-name">Fixed ? requires preparation</div>}
    <TypedParamValue {...props} />
  </>;
}

function TypedParamValue({
  param,
  commitParam,
  curveLibrary,
  gradientLibrary,
  markCollections,
  automation = null,
  linkCurve,
  unlinkCurve,
  linkGradient,
  unlinkGradient
}: {
  param: SequenceEffectParam;
  commitParam: (name: string, value: SequenceEffectParamValue) => Promise<void>;
  curveLibrary: SequenceCurveLibraryItem[];
  gradientLibrary: SequenceGradientLibraryItem[];
  markCollections: SequenceMarkCollection[];
  automation?: ParamAutomationControls | null;
  linkCurve: (name: string, curve: SequenceCurveLibraryItem) => Promise<void>;
  unlinkCurve: (name: string) => Promise<void>;
  linkGradient: (name: string, gradient: SequenceGradientLibraryItem) => Promise<void>;
  unlinkGradient: (name: string) => Promise<void>;
}) {
  const commit = (value: SequenceEffectParamValue) => {
    return commitParam(param.name, value);
  };
  const commitAutomationMapping = async (mapping: SequenceAutomationMapping) => {
    if (automation === null || param.automation === null) return;
    await runGuiEditCommand((request) =>
      commands.applySequenceGuiEdit(request, {
        type: "updateAutomationParamMapping",
        clipId: param.automation?.clipId ?? 0,
        target: automation.target,
        mapping
      })
    );
  };
  const automated = param.automation !== null;

  if (!param.editable && !automated) {
    return (
      <ParamShell name={param.name}>
        <div className="effect-param-unavailable">Unavailable</div>
      </ParamShell>
    );
  }

  const automationActions =
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
  switch (param.value.type) {
    case "int": {
      const mapping = param.automation?.mapping.type === "int" ? param.automation.mapping : null;
      return <ParamShell name={param.name} automated={automated}><ParamValueRow actions={automationActions}>{mapping === null ? <NumberParam key={`${param.name}:${param.value.value}`} value={param.value.value} step={1} disabled={automated} commit={(value) => commit({ type: "int", value: Math.max(0, Math.round(value)) })} /> : <AutomationRangeParam mapping={mapping} step={1} commit={commitAutomationMapping} />}</ParamValueRow></ParamShell>;
    }
    case "float": {
      const mapping = param.automation?.mapping.type === "float" ? param.automation.mapping : null;
      return <ParamShell name={param.name} automated={automated}><ParamValueRow actions={automationActions}>{mapping === null ? <NumberParam key={`${param.name}:${param.value.value}`} value={param.value.value} step={0.05} disabled={automated} commit={(value) => commit({ type: "float", value })} /> : <AutomationRangeParam mapping={mapping} step={0.05} commit={commitAutomationMapping} />}</ParamValueRow></ParamShell>;
    }
    case "bool":
      return (
        <BoolParam
          name={param.name}
          value={param.value.value}
          disabled={automated}
          actions={automationActions}
          commit={(value) => commit({ type: "bool", value })}
        />
      );
    case "color":
      return (
        <ParamShell name={param.name}>
          <ColorPicker
            value={param.value.value}
            label={param.name}
            commit={(value) => commit({ type: "color", value })}
          />
        </ParamShell>
      );
    case "enum":
      return (
        <ParamShell name={param.name} automated={automated}>
          <ParamValueRow actions={automationActions}>
            <select value={param.value.value} disabled={automated} onChange={(event) => void commit({ type: "enum", value: event.currentTarget.value })}>
              {param.options.map((option) => <option key={option} value={option}>{option}</option>)}
            </select>
          </ParamValueRow>
        </ParamShell>
      );
    case "curve":
      return (
        <ParamShell name={param.name} automated={automated}>
        <CurveSourceShell
          name={param.name}
          source={param.curveSource}
          sources={curveLibrary}
          points={normalizeSequenceCurvePoints(param.value.points)}
          commit={(points) => commit({ type: "curve", points })}
          disabled={automated}
          actions={automationActions}
          linkCurve={linkCurve}
          unlinkCurve={unlinkCurve}
          render={(props) => <CurveParam name={param.name} {...props} />}
        />
        </ParamShell>
      );
    case "gradient":
      return (
        <ParamShell name={param.name}>
        <GradientSourceShell
          name={param.name}
          source={param.gradientSource}
          sources={gradientLibrary}
          points={normalizeSequenceGradientStops(param.value.stops)}
          commit={(stops) => commit({ type: "gradient", stops })}
          linkGradient={linkGradient}
          unlinkGradient={unlinkGradient}
          render={(props) => <GradientParam name={param.name} {...props} />}
        />
        </ParamShell>
      );
    case "intArray":
      return <NumberArrayParam name={param.name} values={param.value.values} step={1} commit={(values) => commit({ type: "intArray", values: values.map((value) => Math.max(0, Math.round(value))) })} />;
    case "floatArray":
      return <NumberArrayParam name={param.name} values={param.value.values} step={0.05} commit={(values) => commit({ type: "floatArray", values })} />;
    case "boolArray":
      return <BoolArrayParam name={param.name} values={param.value.values} commit={(values) => commit({ type: "boolArray", values })} />;
    case "colorArray":
      return <ColorArrayParam name={param.name} values={param.value.values} commit={(values) => commit({ type: "colorArray", values })} />;
    case "curveArray":
      return <CurveArrayParam name={param.name} values={param.value.values} commit={(values) => commit({ type: "curveArray", values })} />;
    case "gradientArray":
      return <GradientArrayParam name={param.name} values={param.value.values} commit={(values) => commit({ type: "gradientArray", values })} />;
    case "marks":
      return (
        <ParamShell name={param.name}>
          <select value={param.value.key} onChange={(event) => void commit({ type: "marks", key: event.currentTarget.value })}>
            {markCollections.map((collection) => (
              <option key={collection.key} value={collection.key}>{collection.name}</option>
            ))}
          </select>
        </ParamShell>
      );
  }
}

function ParamShell({ name, automated = false, children }: { name: string; automated?: boolean; children: ReactNode }) {
  return (
    <div className={`effect-param-group ${automated ? "effect-param-automated" : ""}`}>
      <div className="effect-param-name">{name}</div>
      {children}
    </div>
  );
}

function ParamValueRow({ actions, children }: { actions: ReactNode; children: ReactNode }) {
  return (
    <div className="effect-param-value-row">
      <div className="effect-param-value-control">{children}</div>
      {actions !== null && <div className="effect-param-actions">{actions}</div>}
    </div>
  );
}

function BoolParam({
  name,
  value,
  disabled,
  actions,
  commit
}: {
  name: string;
  value: boolean;
  disabled: boolean;
  actions: ReactNode;
  commit: (value: boolean) => Promise<void>;
}) {
  return (
    <div className={`effect-param-group bool-param-group ${disabled ? "effect-param-automated" : ""}`}>
      <div className="bool-param-row">
        <div className="effect-param-name">{name}</div>
        <button
          type="button"
          className="bool-param-switch"
          role="switch"
          aria-label={name}
          aria-checked={value}
          disabled={disabled}
          onClick={() => void commit(!value)}
        >
          <span className="bool-param-switch-track">
            <span className="bool-param-switch-thumb" />
          </span>
        </button>
        {actions !== null && <div className="effect-param-actions">{actions}</div>}
      </div>
    </div>
  );
}

function NumberArrayParam({ name, values, step, commit }: { name: string; values: number[]; step: number; commit: (values: number[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      values={values}
      newValue={() => values[values.length - 1] ?? 0}
      commit={commit}
      render={(value, index) => (
        <input
          type="number"
          step={step}
          defaultValue={value}
          onBlur={(event) => {
            const next = Number(event.currentTarget.value);
            if (Number.isFinite(next)) void commit(replaceAt(values, index, next));
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      )}
    />
  );
}

function automationBindingControl(
  target: SequenceAutomationTarget,
  param: SequenceEffectParam,
  clips: SequenceAutomationClip[],
  canCreateAutomationClip: boolean,
  automationClipChooser: AutomationClipChooser,
  setAutomationClipChooser: (chooser: AutomationClipChooser) => void
) {
  const mapping = defaultAutomationMapping(param);
  if (mapping === null) return null;
  const choosing = automationClipChooser !== null && automationTargetsEqual(automationClipChooser.target, target);
  if (param.automation !== null) {
    return (
      <button
        type="button"
        className="neutral-button icon-button"
        title="Unlink automation"
        onClick={() =>
          void runGuiEditCommand((request) =>
            commands.applySequenceGuiEdit(request, {
              type: "unbindAutomationParam",
              clipId: param.automation?.clipId ?? 0,
              target
            })
          )
        }
      >
        <Link2Off size={THEME_METRICS.iconSizeSmall} />
      </button>
    );
  }
  return (
    <>
      <button
        type="button"
        className="neutral-button icon-button"
        title="Link existing automation"
        disabled={clips.length === 0}
        onClick={() => {
          setAutomationClipChooser({ target, mapping });
        }}
      >
        <Link2 size={THEME_METRICS.iconSizeSmall} />
      </button>
      <button
        type="button"
        className="neutral-button icon-button"
        title="Create automation"
        disabled={!canCreateAutomationClip}
        onClick={() =>
          void runGuiEditCommand((request) =>
            commands.applySequenceGuiEdit(request, {
              type: "createAndBindAutomationClip",
              target,
              mapping
            })
          )
        }
      >
        <Plus size={THEME_METRICS.iconSizeSmall} />
      </button>
      {choosing && (
        <button type="button" className="neutral-button icon-button" title="Cancel choosing" onClick={() => { setAutomationClipChooser(null); }}>
          <X size={THEME_METRICS.iconSizeSmall} />
        </button>
      )}
    </>
  );
}

function defaultAutomationMapping(param: SequenceEffectParam): SequenceAutomationMapping | null {
  switch (param.value.type) {
    case "float":
      return { type: "float", min: 0, max: Math.max(1, param.value.value) };
    case "int":
      return { type: "int", min: 0, max: Math.max(1, param.value.value) };
    case "bool":
      return { type: "bool" };
    case "enum":
      return { type: "enum", values: param.options };
    case "curve":
      return { type: "curve", min: 0, max: 1 };
    case "color":
    case "marks":
    case "gradient":
    case "intArray":
    case "floatArray":
    case "boolArray":
    case "colorArray":
    case "curveArray":
    case "gradientArray":
      return null;
    default:
      return null;
  }
}

function BoolArrayParam({ name, values, commit }: { name: string; values: boolean[]; commit: (values: boolean[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      values={values}
      newValue={() => false}
      commit={commit}
      render={(value, index) => (
        <label className="effect-param-check array-check-row">
          <input type="checkbox" checked={value} onChange={(event) => void commit(replaceAt(values, index, event.currentTarget.checked))} />
          <span>{value ? "true" : "false"}</span>
        </label>
      )}
    />
  );
}

function ColorArrayParam({ name, values, commit }: { name: string; values: string[]; commit: (values: string[]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      values={values}
      newValue={() => values[values.length - 1] ?? CURVE_EDITOR.defaultColor}
      commit={commit}
      render={(value, index) => (
        <ColorPicker label={`${name} ${index + 1}`} value={value} commit={(next) => commit(replaceAt(values, index, next))} />
      )}
    />
  );
}

function CurveArrayParam({ name, values, commit }: { name: string; values: SequenceCurvePoint[][]; commit: (values: SequenceCurvePoint[][]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      values={values}
      newValue={() => values[values.length - 1] ?? [{ time: 0, value: 0 }]}
      commit={commit}
      render={(points, index) => (
        <CurveParam name={`#${index + 1}`} points={normalizeSequenceCurvePoints(points)} commit={(next) => commit(replaceAt(values, index, next))} />
      )}
    />
  );
}

function GradientArrayParam({ name, values, commit }: { name: string; values: SequenceGradientStop[][]; commit: (values: SequenceGradientStop[][]) => Promise<void> }) {
  return (
    <ArrayShell
      name={name}
      values={values}
      newValue={() => values[values.length - 1] ?? [{ time: 0, value: CURVE_EDITOR.defaultColor }]}
      commit={commit}
      render={(points, index) => (
        <GradientParam name={`#${index + 1}`} points={normalizeSequenceGradientStops(points)} commit={(next) => commit(replaceAt(values, index, next))} />
      )}
    />
  );
}

function ArrayShell<T>({
  name,
  values,
  newValue,
  commit,
  render
}: {
  name: string;
  values: T[];
  newValue: () => T;
  commit: (values: T[]) => Promise<void>;
  render: (value: T, index: number) => ReactNode;
}) {
  return (
    <div className="effect-param-group array-param-editor">
      <div className="array-param-header">
        <div className="effect-param-name">{name}</div>
        <button type="button" className="neutral-button" title="Add item" onClick={() => void commit([...values, newValue()])}>
          <Plus size={THEME_METRICS.iconSizeSmall} />
        </button>
      </div>
      {values.map((value, index) => (
        <div key={index} className="array-param-row">
          <div className="array-param-row-main">{render(value, index)}</div>
          <div className="array-param-actions">
            <button type="button" className="neutral-button" title="Move up" disabled={index === 0} onClick={() => void commit(moveArrayItem(values, index, index - 1))}>
              <ArrowUp size={THEME_METRICS.iconSizeSmall} />
            </button>
            <button type="button" className="neutral-button" title="Move down" disabled={index >= values.length - 1} onClick={() => void commit(moveArrayItem(values, index, index + 1))}>
              <ArrowDown size={THEME_METRICS.iconSizeSmall} />
            </button>
            <button type="button" className="neutral-button" title="Remove item" onClick={() => void commit(values.filter((_, itemIndex) => itemIndex !== index))}>
              <Trash2 size={THEME_METRICS.iconSizeSmall} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

function NumberParam({
  value,
  step,
  disabled = false,
  commit
}: {
  value: number;
  step: number;
  disabled?: boolean;
  commit: (value: number) => Promise<void>;
}) {
  const [text, setText] = useState(String(value));
  const lastCommitted = useRef(value);
  const commitText = () => {
    const next = Number(text);
    if (!Number.isFinite(next)) {
      setText(String(value));
      return;
    }
    if (next !== lastCommitted.current) {
      lastCommitted.current = next;
      void commit(next);
    }
  };
  return (
    <label className="effect-param-control-label">
      <input
        type="number"
        step={step}
        value={text}
        disabled={disabled}
        onChange={(event) => { setText(event.currentTarget.value); }}
        onBlur={commitText}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            commitText();
            event.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

function AutomationRangeParam({
  mapping,
  step,
  commit
}: {
  mapping: Extract<SequenceAutomationMapping, { type: "float" } | { type: "int" }>;
  step: number;
  commit: (mapping: SequenceAutomationMapping) => Promise<void>;
}) {
  return (
    <div className="effect-param-automation-range">
      <span>Min</span>
      <NumberParam value={mapping.min} step={step} commit={(min) => commit({ ...mapping, min })} />
      <span>Max</span>
      <NumberParam value={mapping.max} step={step} commit={(max) => commit({ ...mapping, max })} />
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

type CopyAction = "edit" | "flipHorizontal" | "flipVertical";

function CurveSourceShell({
  name,
  source,
  sources,
  points,
  commit,
  disabled = false,
  actions = null,
  linkCurve,
  unlinkCurve,
  render
}: {
  name: string;
  source: SequenceCurveSource | null;
  sources: SequenceCurveLibraryItem[];
  points: EditedSequenceCurvePoint[];
  commit: (points: EditedSequenceCurvePoint[]) => Promise<void>;
  disabled?: boolean;
  actions?: ReactNode;
  linkCurve: (name: string, curve: SequenceCurveLibraryItem) => Promise<void>;
  unlinkCurve: (name: string) => Promise<void>;
  render: (props: CurveEditorProps<EditedSequenceCurvePoint>) => ReactNode;
}) {
  return <LibraryValueShell
    name={name}
    label="curve"
    source={source}
    sources={sources}
    points={points}
    commit={commit}
    disabled={disabled}
    actions={actions}
    link={(curve) => linkCurve(name, curve)}
    unlink={() => unlinkCurve(name)}
    flipVerticalPoints={(current) => current.map((point) => ({ ...point, value: roundCurveValue(1 - point.value) }))}
    render={render}
  />;
}

function GradientSourceShell({
  name,
  source,
  sources,
  points,
  commit,
  linkGradient,
  unlinkGradient,
  render
}: {
  name: string;
  source: SequenceGradientSource | null;
  sources: SequenceGradientLibraryItem[];
  points: EditedSequenceGradientStop[];
  commit: (stops: EditedSequenceGradientStop[]) => Promise<void>;
  linkGradient: (name: string, gradient: SequenceGradientLibraryItem) => Promise<void>;
  unlinkGradient: (name: string) => Promise<void>;
  render: (props: CurveEditorProps<EditedSequenceGradientStop>) => ReactNode;
}) {
  return <LibraryValueShell
    name={name}
    label="gradient"
    source={source}
    sources={sources}
    points={points}
    commit={commit}
    link={(gradient) => linkGradient(name, gradient)}
    unlink={() => unlinkGradient(name)}
    render={render}
  />;
}

type LibraryItem = { moduleId: string; path: string; objectKey: string; displayName: string };
type LibraryReference = {
  type: "library";
  reference: string;
  moduleId: string | null;
  path: string | null;
  objectKey: string | null;
  displayName: string | null;
};

function LibraryValueShell<T extends { time: number }, S extends LibraryItem>({
  name,
  label,
  source,
  sources,
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
  label: "curve" | "gradient";
  source: { type: "inline" } | LibraryReference | null;
  sources: S[];
  points: T[];
  commit: (points: T[]) => Promise<void>;
  disabled?: boolean;
  actions?: ReactNode;
  link: (source: S) => Promise<void>;
  unlink: () => Promise<void>;
  flipVerticalPoints?: (points: T[]) => T[];
  render: (props: CurveEditorProps<T>) => ReactNode;
}) {
  const [pendingCopyAction, setPendingCopyAction] = useState<CopyAction | null>(null);
  const librarySource = source?.type === "library" ? source : null;
  const linked = librarySource !== null;
  const linkedLabel = librarySource?.displayName ?? librarySource?.reference ?? "";
  const availableSources = sources;
  const selectedSourceIndex = linked && librarySource.moduleId !== null && librarySource.path !== null && librarySource.objectKey !== null
    ? availableSources.findIndex((item) =>
      item.moduleId === librarySource.moduleId
      && item.path === librarySource.path
      && item.objectKey === librarySource.objectKey
    )
    : -1;
  const requestEditableCopy = (action: CopyAction) => {
    if (!linked) return;
    setPendingCopyAction(action);
  };
  const flipHorizontal = () => {
    const next = sortCurvePoints(points.map((point) => ({ ...point, time: roundCurveValue(1 - point.time) }))) as T[];
    if (linked) {
      requestEditableCopy("flipHorizontal");
    } else {
      void commit(next);
    }
  };
  const flipVertical = () => {
    if (flipVerticalPoints === undefined) return;
    const next = flipVerticalPoints(points);
    if (linked) {
      requestEditableCopy("flipVertical");
    } else {
      void commit(next);
    }
  };
  const confirmPendingCopyAction = () => {
    const action = pendingCopyAction;
    if (action === null) return;
    if (action === "flipHorizontal") {
      const next = sortCurvePoints(points.map((point) => ({ ...point, time: roundCurveValue(1 - point.time) }))) as T[];
      void commit(next);
    } else if (action === "flipVertical") {
      if (flipVerticalPoints === undefined) return;
      const next = flipVerticalPoints(points);
      void commit(next);
    } else {
      void unlink();
    }
    setPendingCopyAction(null);
  };
  const copyDialogTitle = pendingCopyAction === "flipHorizontal" || pendingCopyAction === "flipVertical"
    ? `Flip ${name} copy?`
    : `Edit ${name} copy?`;
  const copyDialogDescription = pendingCopyAction === "flipHorizontal" || pendingCopyAction === "flipVertical"
    ? `This ${label} is linked from the library. Dawn will make an editable custom copy before applying the flip.`
    : `This ${label} is linked from the library. Dawn will make an editable custom copy so changes do not modify the library ${label}.`;
  return (
    <div className={`param-source-shell ${linked ? "linked" : ""}`}>
      <div className="param-source-row">
        <select
          title={`${name} ${label} source`}
          disabled={disabled}
          value={linked ? `library:${selectedSourceIndex}` : "custom"}
          onChange={(event) => {
            const value = event.currentTarget.value;
            if (value === "custom") {
              if (linked) void unlink();
              return;
            }
            const index = Number(value.replace("library:", ""));
            const source = availableSources[index];
            if (source === undefined) return;
            void link(source);
          }}
        >
          <option value="custom">Custom {label}</option>
          {linked && selectedSourceIndex === -1 && (
            <option value="library:-1">{linkedLabel}</option>
          )}
          {availableSources.map((item, index) => (
            <option key={index} value={`library:${index}`}>
              {item.displayName}
            </option>
          ))}
        </select>
        {actions !== null && <div className="effect-param-actions">{actions}</div>}
      </div>
      {!disabled && (
        <div className="param-source-actions">
          {linked && (
            <button type="button" className="neutral-button icon-button" title="Make editable copy" onClick={() => { requestEditableCopy("edit"); }}>
              <CopyPlus size={THEME_METRICS.iconSizeSmall} />
            </button>
          )}
          <button type="button" className="neutral-button icon-button" title="Flip horizontal" onClick={flipHorizontal}>
            <FlipHorizontal2 size={THEME_METRICS.iconSizeSmall} />
          </button>
          {flipVerticalPoints !== undefined && (
            <button type="button" className="neutral-button icon-button" title="Flip vertical" onClick={flipVertical}>
              <FlipVertical2 size={THEME_METRICS.iconSizeSmall} />
            </button>
          )}
        </div>
      )}
      {render({
        points,
        commit,
        readOnly: disabled || linked,
        ...(disabled ? {} : { requestInlineEdit: () => { requestEditableCopy("edit"); } }),
        showName: false
      })}
      <AlertDialog.Root open={pendingCopyAction !== null} onOpenChange={(open) => { if (!open) setPendingCopyAction(null); }}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="dialog-overlay" />
          <AlertDialog.Content className="dialog-content">
            <AlertDialog.Title>{copyDialogTitle}</AlertDialog.Title>
            <AlertDialog.Description>{copyDialogDescription}</AlertDialog.Description>
            <div className="dialog-actions">
              <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
              <AlertDialog.Action onClick={confirmPendingCopyAction}>Make Copy</AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    </div>
  );
}

function CurveParam({
  name,
  points,
  commit,
  readOnly = false,
  requestInlineEdit,
  showName = true
}: {
  name: string;
  points: EditedSequenceCurvePoint[];
  commit: (points: EditedSequenceCurvePoint[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
  showName?: boolean;
}) {
  const [drafts, setDrafts] = useState(points);
  const draftsRef = useRef(points);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [pointsCollapsed, setPointsCollapsed] = useState(true);
  const svgRef = useRef<SVGSVGElement | null>(null);
  const draggingPoint = useRef<number | null>(null);
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
  const update = (next: EditedSequenceCurvePoint[]) => {
    if (readOnly) {
      requestInlineEdit?.();
      return;
    }
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && Number.isFinite(point.value))) {
      const sorted = sortCurvePoints(next);
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      setSelectedIndex((index) => Math.min(index, sorted.length - 1));
      if (signature !== lastCommittedSignature.current) {
        lastCommittedSignature.current = signature;
        pendingSignature.current = signature;
        void commit(sorted).catch(() => {
          pendingSignature.current = null;
        });
      }
    }
  };
  const setPoint = (index: number, point: EditedSequenceCurvePoint, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestFloatPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    setSelectedIndex(nextIndex);
    if (commitChange) {
      update(next);
    }
    return nextIndex;
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
    setSelectedIndex(Math.min(index, next.length - 1));
  };
  const commitDraftPoint = (index: number) => {
    const point = draftsRef.current[index];
    if (!point) return;
    update(replaceAt(draftsRef.current, index, { time: clamp(point.time, 0, 1), value: point.value }));
  };
  const valueRange = curveValueRange(drafts);
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
            <circle
              key={index}
              className={`float-curve-point ${index === selectedIndex ? "selected" : ""}`}
              cx={x}
              cy={y}
              r={index === selectedIndex ? THEME_METRICS.curvePointRadiusSelected : THEME_METRICS.curvePointRadius}
              tabIndex={0}
              onPointerDown={(event) => {
                event.stopPropagation();
                event.currentTarget.ownerSVGElement?.setPointerCapture(event.pointerId);
                draggingPoint.current = index;
                setSelectedIndex(index);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                deletePoint(index);
              }}
              onFocus={() => { setSelectedIndex(index); }}
            />
          );
        })}
      </svg>
      {!readOnly && (
        <div className="float-curve-points-panel">
            <button
              type="button"
              className="float-curve-points-toggle"
              onClick={() => { setPointsCollapsed((collapsed) => !collapsed); }}
            >
              {pointsCollapsed ? <ChevronRight size={THEME_METRICS.iconSizeExtraSmall} /> : <ChevronRight className="expanded" size={THEME_METRICS.iconSizeExtraSmall} />}
              <span>Points</span>
              <strong>{drafts.length}</strong>
            </button>
            {!pointsCollapsed && (
              <div className="float-curve-point-list">
                {drafts.map((point, index) => (
                  <div
                    key={`${index}:${point.time}:${point.value}`}
                    className={`float-curve-point-row ${index === selectedIndex ? "selected" : ""}`}
                    onPointerDown={() => { setSelectedIndex(index); }}
                  >
                    <label>
                      <span>t</span>
                      <input
                        type="number"
                        min={0}
                        max={1}
                        step={0.01}
                        value={point.time}
                        onFocus={() => { setSelectedIndex(index); }}
                        onChange={(event) => {
                          setPoint(index, { ...point, time: Number(event.currentTarget.value) }, false);
                        }}
                        onBlur={() => { commitDraftPoint(index); }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            commitDraftPoint(index);
                            event.currentTarget.blur();
                          }
                        }}
                      />
                    </label>
                    <label>
                      <span>v</span>
                      <input
                        type="number"
                        step={0.05}
                        value={point.value}
                        onFocus={() => { setSelectedIndex(index); }}
                        onChange={(event) => {
                          setPoint(index, { ...point, value: Number(event.currentTarget.value) }, false);
                        }}
                        onBlur={() => { commitDraftPoint(index); }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            commitDraftPoint(index);
                            event.currentTarget.blur();
                          }
                        }}
                      />
                    </label>
                    <button
                      type="button"
                      className="float-curve-point-delete"
                      title="Delete point"
                      disabled={drafts.length <= 1}
                      onClick={() => { deletePoint(index); }}
                    >
                      <Minus size={THEME_METRICS.iconSizeSmall} />
                    </button>
                  </div>
                ))}
              </div>
            )}
        </div>
      )}
    </div>
  );
}

function GradientParam({
  name,
  points,
  commit,
  readOnly = false,
  requestInlineEdit,
  showName = true
}: {
  name: string;
  points: EditedSequenceGradientStop[];
  commit: (points: EditedSequenceGradientStop[]) => Promise<void>;
  readOnly?: boolean;
  requestInlineEdit?: () => void;
  showName?: boolean;
}) {
  const [drafts, setDrafts] = useState(points);
  const draftsRef = useRef(points);
  const [stopPickerRequest, setStopPickerRequest] = useState<{ index: number; key: number } | null>(null);
  const stopPickerRequestKey = useRef(0);
  const gradientRef = useRef<HTMLDivElement | null>(null);
  const draggingPoint = useRef<{ index: number; pointerId: number; startClientX: number; moved: boolean } | null>(null);
  const lastCommittedValues = useRef(points.map((point) => point.value.toLowerCase()));
  const pointsSignature = curvePointsSignature(points);
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
    lastCommittedValues.current = points.map((point) => point.value.toLowerCase());
  }, [points, pointsSignature]);
  const update = (next: EditedSequenceGradientStop[]) => {
    if (readOnly) {
      requestInlineEdit?.();
      return;
    }
    if (next.length > 0 && next.every((point) => Number.isFinite(point.time) && isHexColor(point.value))) {
      const sorted = sortCurvePoints(next).map((point) => ({ ...point, value: point.value.toLowerCase() }));
      const signature = curvePointsSignature(sorted);
      setDrafts(sorted);
      draftsRef.current = sorted;
      lastCommittedValues.current = sorted.map((point) => point.value);
      pendingSignature.current = signature;
      void commit(sorted).catch(() => {
        pendingSignature.current = null;
      });
    }
  };
  const setPoint = (index: number, point: EditedSequenceGradientStop, commitChange: boolean) => {
    const next = sortCurvePoints(replaceAt(draftsRef.current, index, point));
    const nextIndex = nearestColorPointIndex(next, point);
    setDrafts(next);
    draftsRef.current = next;
    if (commitChange) {
      update(next);
    }
    return nextIndex;
  };
  const deletePoint = (index: number) => {
    if (draftsRef.current.length <= 1) return;
    const next = draftsRef.current.filter((_, pointIndex) => pointIndex !== index);
    update(next);
  };
  const pointFromPointer = (event: PointerEvent<HTMLElement>, color: string): EditedSequenceGradientStop => {
    const rect = gradientRef.current?.getBoundingClientRect();
    if (rect === undefined) return { time: 0, value: color };
    return {
      time: roundCurveValue(clamp((event.clientX - rect.left) / Math.max(1, rect.width), 0, 1)),
      value: color
    };
  };
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
  const gradient = gradientCss(drafts);
  return (
    <div className="effect-param-group color-curve-editor">
      {showName && <div className="effect-param-name">{name}</div>}
      <div
        ref={gradientRef}
        className="color-curve-gradient"
        style={{ background: gradient }}
        onPointerDown={(event) => {
          if (readOnly) {
            requestInlineEdit?.();
            return;
          }
          if (event.target !== event.currentTarget) return;
          const previous = draftsRef.current[draftsRef.current.length - 1]?.value ?? CURVE_EDITOR.defaultColor;
          const point = pointFromPointer(event, previous);
          update([...draftsRef.current, point]);
        }}
      >
        {!readOnly && drafts.map((point, index) => {
          return (
            <span
              key={index}
              className="color-curve-stop"
              draggable={false}
              style={{ left: `${point.time * 100}%` }}
              onPointerDown={(event) => {
                if (!event.currentTarget.contains(event.target as Node)) return;
                if (event.button !== 0) return;
                event.preventDefault();
                event.stopPropagation();
                startStopDrag(index, event.pointerId, event.clientX);
              }}
              onDragStart={(event) => {
                event.preventDefault();
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                event.stopPropagation();
                deletePoint(index);
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
                value={isHexColor(point.value) ? point.value : (points[index]?.value ?? CURVE_EDITOR.defaultColor)}
                label={`${name} stop ${index + 1} color`}
                commit={(value) => {
                  setPoint(index, { ...point, value }, true);
                  return Promise.resolve();
                }}
              />
            </span>
          );
        })}
      </div>
    </div>
  );
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

function curveValueRange(points: EditedSequenceCurvePoint[]) {
  const values = points.map((point) => point.value).filter(Number.isFinite);
  const min = Math.min(0, ...values);
  const max = Math.max(1, ...values);
  if (Math.abs(max - min) < CURVE_EDITOR.flatRangeEpsilon) return { min: min - 0.5, max: max + 0.5 };
  return { min, max };
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

function gradientCss(points: EditedSequenceGradientStop[]) {
  const stops = sortCurvePoints(points)
    .filter((point) => isHexColor(point.value))
    .map((point) => `${point.value} ${clamp(point.time, 0, 1) * 100}%`);
  if (stops.length === 0) return CURVE_EDITOR.emptyGradient;
  if (stops.length === 1) return stops[0]?.split(" ")[0] ?? CURVE_EDITOR.emptyGradient;
  return `linear-gradient(90deg, ${stops.join(", ")})`;
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
    .filter((point) => isHexColor(point.value))
    .filter((point) => Number.isFinite(point.time))
    .map((point) => ({ time: clamp(point.time, 0, 1), value: point.value.toLowerCase() }));
  return normalized.length > 0 ? normalized : [{ time: 0, value: CURVE_EDITOR.defaultColor }];
}

function isHexColor(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}
