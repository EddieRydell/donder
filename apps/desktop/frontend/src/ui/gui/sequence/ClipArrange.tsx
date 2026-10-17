import * as ContextMenu from "@radix-ui/react-context-menu";
import * as Dialog from "@radix-ui/react-dialog";
import { ChevronRight } from "lucide-react";
import { Fragment, useContext, useState, type ReactNode } from "react";
import type { SequenceClipArrangement, SequenceClipRef } from "../../../editor/types";
import { THEME_METRICS } from "../../../theme";
import { OverlayPortal } from "../../OverlayPortal";

type ReferenceArrangement = Extract<SequenceClipArrangement, { reference: SequenceClipRef }>["type"];
type SelectionArrangement = Exclude<SequenceClipArrangement, { reference: SequenceClipRef }>;

/** Align items, grouped; each lines the selection up with the right-clicked clip. */
const ALIGN_ITEMS: { type: ReferenceArrangement; label: string }[][] = [
  [
    { type: "alignStarts", label: "Align Starts" },
    { type: "alignEnds", label: "Align Ends" },
    { type: "alignBoth", label: "Align Starts and Ends" },
    { type: "matchDuration", label: "Match Duration" }
  ],
  [
    { type: "moveStarts", label: "Align Starts (Keep Length)" },
    { type: "moveEnds", label: "Align Ends (Keep Length)" },
    { type: "alignCenters", label: "Align Centers" }
  ],
  [
    { type: "startAfter", label: "Start After This Clip" },
    { type: "endBefore", label: "End Before This Clip" }
  ]
];

const MARK_ITEMS: { type: "quantizeToMarks" | "stretchToMarks"; label: string }[] = [
  { type: "quantizeToMarks", label: "Quantize to Marks" },
  { type: "stretchToMarks", label: "Stretch to Marks" }
];

const ROW_ITEMS: { arrangement: SelectionArrangement; label: string }[] = [
  { arrangement: { type: "fillGaps" }, label: "Fill Gaps" },
  { arrangement: { type: "buttTogether" }, label: "Butt Together" },
  { arrangement: { type: "distributeEvenly" }, label: "Distribute Evenly" },
  { arrangement: { type: "equalizeDurations" }, label: "Equalize Durations" }
];

/**
 * The Align and Arrange submenus of a clip's context menu. Align needs another selected clip to
 * move; the mark items use the visible mark collections.
 */
export function ClipArrangeMenus({ reference, canAlign, markCollections, onArrange, onCascade }: {
  reference: SequenceClipRef;
  canAlign: boolean;
  markCollections: string[];
  onArrange: (arrangement: SequenceClipArrangement) => void;
  onCascade: () => void;
}) {
  const container = useContext(OverlayPortal);
  const submenu = (label: string, disabled: boolean, items: ReactNode) => (
    <ContextMenu.Sub>
      <ContextMenu.SubTrigger className="menu-item" disabled={disabled}>
        {label} <ChevronRight size={THEME_METRICS.iconSizeSmall} aria-hidden />
      </ContextMenu.SubTrigger>
      <ContextMenu.Portal container={container}>
        <ContextMenu.SubContent className="menu-content">{items}</ContextMenu.SubContent>
      </ContextMenu.Portal>
    </ContextMenu.Sub>
  );
  return <>
    {submenu("Align", !canAlign, ALIGN_ITEMS.map((group, index) => (
      <Fragment key={index}>
        {index > 0 && <ContextMenu.Separator className="menu-separator" />}
        {group.map((item) => (
          <ContextMenu.Item key={item.type} className="menu-item" onSelect={() => { onArrange({ type: item.type, reference }); }}>
            {item.label}
          </ContextMenu.Item>
        ))}
      </Fragment>
    )))}
    {submenu("Arrange", false, <>
      {MARK_ITEMS.map((item) => (
        <ContextMenu.Item key={item.type} className="menu-item" disabled={markCollections.length === 0} onSelect={() => { onArrange({ type: item.type, markCollections }); }}>
          {item.label}
        </ContextMenu.Item>
      ))}
      <ContextMenu.Separator className="menu-separator" />
      {ROW_ITEMS.map((item) => (
        <ContextMenu.Item key={item.arrangement.type} className="menu-item" onSelect={() => { onArrange(item.arrangement); }}>
          {item.label}
        </ContextMenu.Item>
      ))}
      <ContextMenu.Separator className="menu-separator" />
      <ContextMenu.Item className="menu-item" onSelect={onCascade}>Cascade…</ContextMenu.Item>
    </>)}
  </>;
}

const MS_PER_SECOND = 1000;
const DEFAULT_CASCADE_STEP_MS = 50;

/** Cascade's step and direction. */
export function CascadeDialog({ onCascade, onClose }: { onCascade: (stepSeconds: number, reverse: boolean) => void; onClose: () => void }) {
  const [stepMs, setStepMs] = useState(DEFAULT_CASCADE_STEP_MS);
  const [reverse, setReverse] = useState(false);
  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open) onClose(); }}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content clip-options-dialog">
          <Dialog.Title>Cascade clips</Dialog.Title>
          <Dialog.Description>
            Offset each lane&apos;s selected clips by one more step than the lane above, turning a column of clips into a chase.
          </Dialog.Description>
          <fieldset className="clip-options">
            <label htmlFor="cascade-step">Step</label>
            <label className="clip-options-value">
              <input
                id="cascade-step"
                type="number"
                step="any"
                value={stepMs}
                onChange={(event) => {
                  const value = event.currentTarget.valueAsNumber;
                  if (Number.isFinite(value)) setStepMs(value);
                }}
              />
              ms
            </label>
            <label><input type="checkbox" checked={reverse} onChange={(event) => { setReverse(event.currentTarget.checked); }} />Bottom lane first</label>
            <span />
          </fieldset>
          <div className="dialog-actions">
            <button type="button" onClick={onClose}>Cancel</button>
            <button type="button" onClick={() => { onCascade(stepMs / MS_PER_SECOND, reverse); }}>Cascade</button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
