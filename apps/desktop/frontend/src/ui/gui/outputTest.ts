import { Lightbulb, Repeat, Square } from "lucide-react";
import { commands } from "../../api";
import { runSnapshotCommand, useAppStore } from "../../store";
import type { OutputTestPattern, OutputTestTarget } from "../../types";
import type { MenuEntry } from "../kit";

const FULL = 255;
const PATTERNS: { label: string; pattern: OutputTestPattern }[] = [
  { label: "Count", pattern: { type: "count" } },
  { label: "Color order", pattern: { type: "order" } },
  { label: "Chase", pattern: { type: "chase" } },
  { label: "Rise", pattern: { type: "rise" } },
  { label: "Red", pattern: { type: "solid", red: FULL, green: 0, blue: 0 } },
  { label: "Green", pattern: { type: "solid", red: 0, green: FULL, blue: 0 } },
  { label: "Blue", pattern: { type: "solid", red: 0, green: 0, blue: FULL } },
  { label: "White", pattern: { type: "solid", red: FULL, green: FULL, blue: FULL } }
];

/** Stream a pattern on what `target` denotes in the open document until output stops, from each fixture's last pixel when `reversed`; a refusal shows in the status bar. */
export function startOutputTest(target: OutputTestTarget, pattern: OutputTestPattern, reversed = false) {
  const request = useAppStore.getState().guiRequest;
  if (request === null) return;
  void runSnapshotCommand(() => commands.startOutputTest(request, target, pattern, reversed)).catch(() => undefined);
}

export function stopOutputTest() {
  void runSnapshotCommand(() => commands.setLiveOutputActive(false)).catch(() => undefined);
}

export const useOutputTesting = () => useAppStore((state) => state.snapshot?.liveOutput.state === "testing");

/** The patterns to test `target` with, as menu entries. */
export const patternEntries = (target: OutputTestTarget, disabled: boolean, reversed = false): MenuEntry[] =>
  PATTERNS.map(({ label, pattern }): MenuEntry => ({ type: "action", label, disabled, onSelect: () => { startOutputTest(target, pattern, reversed); } }));

/** "Test" and "Test reversed" submenus for `target`, and "Stop test" while one runs. */
export function outputTestEntries(target: OutputTestTarget, disabled: boolean, testing: boolean): MenuEntry[] {
  return [
    { type: "submenu", label: "Test", icon: Lightbulb, disabled, entries: patternEntries(target, disabled) },
    { type: "submenu", label: "Test reversed", icon: Repeat, disabled, entries: patternEntries(target, disabled, true) },
    ...(testing ? [{ type: "action", label: "Stop test", icon: Square, onSelect: stopOutputTest }] satisfies MenuEntry[] : [])
  ];
}
