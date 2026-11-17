import { RadioTower } from "lucide-react";
import { commands } from "../api";
import { runSnapshotCommand } from "../store";
import type { LiveOutputSnapshot } from "../types";
import { IconButton } from "./kit";

/** Turns live output on or off; red while it, or a controller it drives, has failed, with each failure in its tooltip. */
export function LiveOutputButton({ liveOutput }: { liveOutput: LiveOutputSnapshot }) {
  const errors = [...new Set([
    ...liveOutput.controllers.flatMap((controller) => controller.lastError === null ? [] : [controller.lastError]),
    ...(liveOutput.lastError === null ? [] : [liveOutput.lastError])
  ])];
  return <IconButton
    icon={RadioTower}
    label={errors.length === 0 ? `Live output: ${liveOutput.state}` : errors.join("; ")}
    pressed={liveOutput.enabled}
    failed={errors.length > 0}
    onClick={() => void runSnapshotCommand(() => commands.setLiveOutputActive(!liveOutput.enabled))}
  />;
}
