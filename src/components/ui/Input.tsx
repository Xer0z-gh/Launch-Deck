import { forwardRef, type InputHTMLAttributes } from "react";

import { cn } from "@/lib/cn";

/**
 * The iOS text field: a system-fill capsule with no border.
 *
 * iOS does not outline text fields. It seats them in a translucent fill, and
 * the focus state deepens the fill and adds the tint ring rather than
 * changing a border colour -- a 1px border tint is not a visible focus
 * indicator on either theme, which is why the previous version had to add a
 * ring on top of it anyway.
 */
export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input
      ref={ref}
      // Text inputs here are search boxes, names and commands -- never prose,
      // and never something a browser should offer to autofill.
      autoComplete="off"
      spellCheck={false}
      className={cn(
        "h-[34px] w-full rounded-capsule border-0 bg-fill-3 px-3.5",
        "text-[15px] text-ink-strong",
        "placeholder:text-ink/45",
        "transition-colors duration-[140ms] ease-[var(--ease-standard)]",
        "hover:bg-fill-2",
        "focus-visible:bg-fill-2 focus-visible:outline-2 focus-visible:outline-offset-0",
        "focus-visible:outline-accent",
        "disabled:opacity-50",
        className,
      )}
      {...props}
    />
  ),
);
Input.displayName = "Input";
