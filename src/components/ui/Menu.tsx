import * as RadixMenu from "@radix-ui/react-dropdown-menu";
import type { ReactNode } from "react";

import { cn } from "@/lib/cn";

export interface MenuItemSpec {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  /** Draw a separator above this item. */
  section?: boolean;
}

export interface MenuProps {
  trigger: ReactNode;
  items: MenuItemSpec[];
  align?: "start" | "end";
}

/** Dropdown menu on Radix primitives: keyboard nav and typeahead for free. */
export function Menu({ trigger, items, align = "end" }: MenuProps) {
  return (
    <RadixMenu.Root>
      <RadixMenu.Trigger asChild>{trigger}</RadixMenu.Trigger>
      <RadixMenu.Portal>
        <RadixMenu.Content
          align={align}
          sideOffset={4}
          collisionPadding={8}
          className={cn(
            "z-50 min-w-44 rounded-panel border border-rule bg-overlay p-1",
            "[box-shadow:var(--hairline-top-strong),0_16px_32px_-8px_rgb(0_0_0/0.55)]",
            // The menu is taller than a small window: without a ceiling Radix
            // overflows items past the viewport edge, where they cannot be
            // clicked at all (measured: "Build prompt…" at y = -62 in a 461px
            // window). Capped to the collision-aware available height, the
            // menu scrolls instead.
            "max-h-[var(--radix-dropdown-menu-content-available-height)] overflow-y-auto",
          )}
        >
          {items.map((item, index) => (
            <div key={item.label}>
              {item.section && index > 0 && (
                <RadixMenu.Separator className="mx-1 my-1 h-px bg-rule" />
              )}
              <RadixMenu.Item
                disabled={item.disabled ?? false}
                onSelect={item.onSelect}
                className={cn(
                  "flex cursor-default select-none items-center gap-2 rounded-control",
                  "px-2 py-1.5 text-xs outline-none",
                  "data-[highlighted]:bg-raised",
                  "data-[disabled]:opacity-40",
                  item.danger ? "text-signal" : "text-ink/85",
                )}
              >
                {item.icon}
                {item.label}
              </RadixMenu.Item>
            </div>
          ))}
        </RadixMenu.Content>
      </RadixMenu.Portal>
    </RadixMenu.Root>
  );
}
