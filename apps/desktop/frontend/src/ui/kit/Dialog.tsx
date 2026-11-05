import * as RadixDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { forwardRef, useContext, type ReactNode } from "react";
import { THEME_METRICS } from "../../theme";
import { OverlayPortal } from "../OverlayPortal";

type DialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  /** The buttons at the bottom right, primary last. */
  actions?: ReactNode;
  /** `full` is a whole-window editor surface whose body fills the dialog without padding. */
  size?: "sm" | "md" | "lg" | "full";
  /** Focus and Escape handling for a dialog that hosts its own editor. */
  focus?: Pick<RadixDialog.DialogContentProps, "onOpenAutoFocus" | "onCloseAutoFocus" | "onEscapeKeyDown">;
  className?: string;
  children?: ReactNode;
};

/**
 * The app's dialog: a flat panel with a title row. Use it only for work that needs the user's whole
 * attention; everything else edits in place. The ref is the dialog panel.
 */
export const Dialog = forwardRef<HTMLDivElement, DialogProps>(function Dialog(
  { open, onOpenChange, title, actions, size = "sm", focus, className, children },
  ref
) {
  const container = useContext(OverlayPortal);
  return <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
    <RadixDialog.Portal container={container}>
      <RadixDialog.Overlay className="kit-dialog-overlay" />
      <RadixDialog.Content ref={ref} className={["kit-dialog", `kit-dialog-${size}`, className].filter(Boolean).join(" ")} aria-describedby={undefined} {...focus}>
        <div className="kit-dialog-title">
          <RadixDialog.Title>{title}</RadixDialog.Title>
          <RadixDialog.Close className="kit-dialog-close" aria-label="Close"><X size={THEME_METRICS.iconSizeSmall} /></RadixDialog.Close>
        </div>
        {children !== undefined && <div className="kit-dialog-body">{children}</div>}
        {actions !== undefined && <div className="kit-dialog-actions">{actions}</div>}
      </RadixDialog.Content>
    </RadixDialog.Portal>
  </RadixDialog.Root>;
});
