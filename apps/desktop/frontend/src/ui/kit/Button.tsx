import type { LucideIcon } from "lucide-react";
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import type { CommandId } from "../../types";
import { THEME_METRICS } from "../../theme";
import { Tip } from "./Tip";

type ButtonVariant = "default" | "primary" | "ghost" | "danger";

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title"> & {
  variant?: ButtonVariant;
  icon?: LucideIcon;
  /** Tooltip text; the command's binding is appended. */
  tip?: string;
  command?: CommandId;
  children?: ReactNode;
};

/** A labelled command button. */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "default", icon: Icon, tip, command, className, children, type = "button", ...rest },
  ref
) {
  const button = <button
    ref={ref}
    type={type}
    className={["kit-button", `kit-button-${variant}`, className].filter(Boolean).join(" ")}
    {...rest}
  >
    {Icon !== undefined && <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />}
    {children}
  </button>;
  return tip === undefined ? button : <Tip label={tip} command={command}>{button}</Tip>;
});

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title" | "aria-label" | "children"> & {
  icon: LucideIcon;
  /** The action's name: the accessible label and tooltip. */
  label: string;
  command?: CommandId;
  /** Present on toggles: whether the option is on. */
  pressed?: boolean;
  danger?: boolean;
  /** A count or marker drawn over the icon, such as a problem count. */
  badge?: ReactNode;
};

/** An icon-only button whose tooltip names it and shows its shortcut. */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon: Icon, label, command, pressed, danger = false, badge, className, type = "button", ...rest },
  ref
) {
  return <Tip label={label} command={command}>
    <button
      ref={ref}
      type={type}
      aria-label={label}
      aria-pressed={pressed}
      className={["kit-icon-button", danger ? "kit-danger" : null, className].filter(Boolean).join(" ")}
      {...rest}
    >
      <Icon size={THEME_METRICS.iconSizeSmall} aria-hidden />
      {badge}
    </button>
  </Tip>;
});
