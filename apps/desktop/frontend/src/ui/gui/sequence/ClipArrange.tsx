import { useState } from "react";
import type { SequenceClipArrangement, SequenceClipRef } from "../../../types";
import { Button, ContextMenuEntries, Dialog, Field, NumberField, Toggle, type MenuEntry } from "../../kit";

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

type ClipArrangeProps = {
  reference: SequenceClipRef;
  canAlign: boolean;
  markCollections: string[];
  onArrange: (arrangement: SequenceClipArrangement) => void;
  onCascade: () => void;
};

/**
 * The Align and Arrange submenus of a clip's context menu. Align needs another selected clip to
 * move; the mark items use the visible mark collections.
 */
export function clipArrangeEntries({ reference, canAlign, markCollections, onArrange, onCascade }: ClipArrangeProps): MenuEntry[] {
  const separator: MenuEntry = { type: "separator" };
  return [
    {
      type: "submenu",
      label: "Align",
      disabled: !canAlign,
      entries: ALIGN_ITEMS.flatMap((group, index): MenuEntry[] => [
        ...(index > 0 ? [separator] : []),
        ...group.map((item): MenuEntry => ({ type: "action", label: item.label, onSelect: () => { onArrange({ type: item.type, reference }); } }))
      ])
    },
    {
      type: "submenu",
      label: "Arrange",
      entries: [
        ...MARK_ITEMS.map((item): MenuEntry => ({ type: "action", label: item.label, disabled: markCollections.length === 0, onSelect: () => { onArrange({ type: item.type, markCollections }); } })),
        separator,
        ...ROW_ITEMS.map((item): MenuEntry => ({ type: "action", label: item.label, onSelect: () => { onArrange(item.arrangement); } })),
        separator,
        { type: "action", label: "Cascade…", onSelect: onCascade }
      ]
    }
  ];
}

/** `clipArrangeEntries` inside an open context menu. */
export function ClipArrangeMenus(props: ClipArrangeProps) {
  return <ContextMenuEntries entries={clipArrangeEntries(props)} />;
}

const MS_PER_SECOND = 1000;
const DEFAULT_CASCADE_STEP_MS = 50;

/** Cascade's step and direction: each lane's selected clips move one more step than the lane above. */
export function CascadeDialog({ onCascade, onClose }: { onCascade: (stepSeconds: number, reverse: boolean) => void; onClose: () => void }) {
  const [stepMs, setStepMs] = useState(DEFAULT_CASCADE_STEP_MS);
  const [reverse, setReverse] = useState(false);
  return (
    <Dialog
      open
      onOpenChange={(open) => { if (!open) onClose(); }}
      title="Cascade clips"
      actions={<>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="primary" onClick={() => { onCascade(stepMs / MS_PER_SECOND, reverse); }}>Cascade</Button>
      </>}
    >
      <Field label="Step" tip="Each lane starts this much later than the lane above">
        <NumberField name="Step" value={stepMs} step={1} unit="ms" defaultValue={DEFAULT_CASCADE_STEP_MS} onCommit={setStepMs} />
      </Field>
      <Field label="Bottom first">
        <Toggle name="Bottom lane first" checked={reverse} onChange={setReverse} />
      </Field>
    </Dialog>
  );
}
