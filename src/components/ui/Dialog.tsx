import * as React from "react";
import * as RadixDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "@/lib/cn";
import { Button } from "@/components/ui/Button";

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  /** One line under the title explaining what this dialog does. */
  description?: string;
  children: ReactNode;
  width?: "md" | "lg";
}

/** Modal dialog on Radix primitives: focus trap, Esc, aria wiring for free. */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  width = "md",
}: DialogProps) {
  const openerRef = React.useRef<HTMLElement | null>(null);
  React.useEffect(() => {
    if (open) {
      openerRef.current =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
  }, [open]);

  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-40 bg-black/50" />
        <RadixDialog.Content
          // Send focus somewhere real when the dialog closes.
          //
          // Radix restores focus to whatever was focused when the dialog
          // opened. Both of these dialogs open from an item inside a dropdown
          // menu, and that item is unmounted by the time the dialog closes --
          // so Radix has nothing to restore to and focus lands on `<body>`.
          // Measured: after Escape, `document.activeElement` was BODY, meaning
          // the next Tab starts over from the top of the document and the
          // keyboard user has lost their place entirely.
          //
          // `[data-dialog-return]` marks the control that stands in for "where
          // you were" -- the toolbar's Add button, which is what opened the
          // menu in the first place.
          onCloseAutoFocus={(event) => {
            // Prefer the element that was focused when the dialog opened --
            // for a row dialog that is the row's own controls, and sending a
            // keyboard user back to the toolbar instead strands them at the
            // top of the app, far from the row they were working.
            const opener = openerRef.current;
            if (opener?.isConnected) {
              event.preventDefault();
              opener.focus();
              return;
            }
            const target = document.querySelector<HTMLElement>("[data-dialog-return]");
            if (!target) return; // let Radix do whatever it would have done
            event.preventDefault();
            target.focus();
          }}
          className={cn(
            "fixed left-1/2 top-1/2 z-50 -translate-x-1/2 -translate-y-1/2",
            "rounded-modal border border-rule bg-panel shadow-2xl shadow-black/40",
            "flex max-h-[85vh] w-[92vw] flex-col",
            width === "md" ? "max-w-lg" : "max-w-2xl",
          )}
        >
          <header className="flex items-start justify-between gap-4 border-b border-rule px-5 pb-3 pt-4">
            <div className="min-w-0">
              <RadixDialog.Title className="text-sm font-semibold text-ink">
                {title}
              </RadixDialog.Title>
              {description ? (
                <RadixDialog.Description className="mt-0.5 text-xs text-ink/55">
                  {description}
                </RadixDialog.Description>
              ) : (
                // Radix warns when a description is absent; suppress with an
                // explicitly empty one rather than shipping the warning.
                <RadixDialog.Description className="sr-only">
                  {title}
                </RadixDialog.Description>
              )}
            </div>
            <RadixDialog.Close asChild>
              <Button size="icon" aria-label="Close dialog">
                <X size={14} />
              </Button>
            </RadixDialog.Close>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-4">
            {children}
          </div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
