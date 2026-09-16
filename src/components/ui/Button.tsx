import { forwardRef, type ButtonHTMLAttributes } from "react";

import { cn } from "@/lib/cn";

type Variant = "primary" | "ghost" | "danger" | "outline";
type Size = "sm" | "md" | "icon";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

/**
 * iOS control styles.
 *
 * Apple ships four button treatments and this app needs exactly four, so
 * the existing names map onto them rather than inventing a parallel set:
 *
 *   primary -> Filled: the tint as a background, white label. One per view.
 *   outline -> Tinted: the tint at low opacity behind the tint-coloured
 *              label. This is the workhorse on iOS 26; it reads as a control
 *              without competing with the filled one.
 *   ghost   -> Plain: label only until touched. Toolbars and list rows.
 *   danger  -> Plain in systemRed.
 *
 * Every one is a capsule. That is the single loudest signal of the current
 * iOS look -- rounded rectangles read as iOS 15, and no amount of correct
 * colour fixes it.
 */
const variantClasses: Record<Variant, string> = {
  primary:
    "bg-accent text-on-accent font-semibold hover:bg-accent-hover active:bg-accent-dim " +
    "disabled:bg-fill-3 disabled:text-ink/30",
  // Tinted: the tint carries both the fill and the label, so the control is
  // legible as "actionable" without a border.
  outline:
    "bg-accent/15 text-accent font-medium hover:bg-accent/25 active:bg-accent/30 " +
    "disabled:bg-fill-4 disabled:text-ink/30",
  // Neutral, not tinted. iOS plain buttons ARE tinted -- but iOS shows two or
  // three per screen, and this app shows two per row across forty rows. At
  // that density the tint stops meaning "action" and becomes the background
  // colour of the app, which is exactly the oversaturation Tanner called out.
  ghost:
    "bg-transparent text-ink/70 hover:bg-fill-4 hover:text-ink-strong active:bg-fill-3 disabled:text-ink/25",
  danger:
    "bg-transparent text-signal hover:bg-signal/12 active:bg-signal/18 disabled:text-ink/30",
};

/**
 * iOS control heights. 44pt is the touch minimum; this is a pointer-driven
 * desktop app with a dense list, so the ladder is compact-but-legal: every
 * size still clears the 24px pointer floor, and `md` matches the 34pt height
 * of a standard iPadOS toolbar control.
 */
const sizeClasses: Record<Size, string> = {
  sm: "h-7 px-3 text-[13px] gap-1.5",
  md: "h-[34px] px-4 text-[15px] gap-2",
  icon: "h-[30px] w-[30px] p-0",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "ghost", size = "md", type, ...props }, ref) => (
    <button
      ref={ref}
      type={type ?? "button"}
      className={cn(
        "inline-flex select-none items-center justify-center rounded-capsule",
        "whitespace-nowrap transition-[background-color,color,transform]",
        "duration-[140ms] ease-[var(--ease-standard)]",
        // iOS controls shrink slightly under the finger rather than
        // changing shade alone.
        "active:scale-[0.96]",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
        "disabled:cursor-not-allowed disabled:active:scale-100",
        variantClasses[variant],
        sizeClasses[size],
        className,
      )}
      {...props}
    />
  ),
);
Button.displayName = "Button";
