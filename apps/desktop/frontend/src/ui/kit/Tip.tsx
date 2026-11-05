import * as Tooltip from "@radix-ui/react-tooltip";
import { useContext, type ReactNode } from "react";
import { shortcutLabel } from "../../commandRegistry";
import type { CommandId } from "../../types";
import { THEME_METRICS } from "../../theme";
import { OverlayPortal } from "../OverlayPortal";

/** The app's single tooltip delay; mounted once at the root. */
export function TipProvider({ children }: { children: ReactNode }) {
  return <Tooltip.Provider delayDuration={THEME_METRICS.tooltipDelayMs}>{children}</Tooltip.Provider>;
}

/** A tooltip on one control. The trigger must accept a ref. */
export function Tip({ label, command, children }: { label: ReactNode; command?: CommandId | undefined; children: ReactNode }) {
  const container = useContext(OverlayPortal);
  const keys = command === undefined ? undefined : shortcutLabel(command);
  return <Tooltip.Root>
    <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
    <Tooltip.Portal container={container}>
      <Tooltip.Content className="kit-tip" side="bottom" sideOffset={THEME_METRICS.popoverOffset} collisionPadding={THEME_METRICS.popoverGap}>
        {label}
        {keys !== undefined && <span className="kit-tip-keys">{keys}</span>}
      </Tooltip.Content>
    </Tooltip.Portal>
  </Tooltip.Root>;
}
