import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { THEME_METRICS } from "../../theme";
import { Tip } from "./Tip";

/** A small state marker, an icon and/or a count or word, that `tip` explains on hover. */
export function Badge({ icon: Icon, tip, children }: { icon?: LucideIcon; tip: string; children?: ReactNode }) {
  return <Tip label={tip}>
    <span className="kit-badge" role="img" aria-label={tip}>
      {Icon !== undefined && <Icon size={THEME_METRICS.iconSizeExtraSmall} aria-hidden />}
      {children}
    </span>
  </Tip>;
}
