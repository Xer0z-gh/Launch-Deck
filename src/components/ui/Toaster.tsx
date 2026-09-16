import { X } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { DURATION, usePresenceList } from "@/lib/presence";
import { useUi } from "@/state/ui";

/** Defined outside the component so the identity is stable across renders --
 *  `usePresenceList` takes it as a dependency. */
const toastKey = (t: { id: number }) => String(t.id);

/** Bottom-right toast stack. Errors persist 6s; everything is dismissible. */
export function Toaster() {
  const toasts = useUi((s) => s.toasts);
  const dismiss = useUi((s) => s.dismissToast);

  // Dismissed toasts stay rendered, marked closed, until their exit has run --
  // otherwise they blink out, which reads as a glitch rather than a dismissal.
  const shown = usePresenceList(toasts, toastKey, DURATION.fast);

  // The container stays mounted even when empty: unmounting it would tear down
  // the aria-live region, and a live region that appears at the same moment as
  // its first message is not reliably announced.
  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-96 max-w-[90vw] flex-col gap-2"
    >
      {shown.map(({ item: toast, open }) => (
        <div
          key={toast.id}
          // The stack reflows instantly when one is dismissed rather than
          // sliding the survivors up. Animating that needs layout projection,
          // which for a two-item stack is not worth its weight -- Vantage
          // already established projection as the expensive part of a motion
          // runtime.
          role={toast.kind === "error" ? "alert" : "status"}
          className={cn(
            "pop-enter flex items-start gap-2 rounded-panel border bg-overlay px-3 py-2.5",
            "[box-shadow:var(--hairline-top-strong),0_12px_24px_-6px_rgb(0_0_0/0.5)]",
            toast.kind === "error" ? "border-signal/40" : "border-rule",
            // A toast on its way out stops accepting clicks: it is already gone
            // from the store, so its dismiss button would do nothing while
            // still looking pressable, and it must not intercept a click aimed
            // at whatever is now underneath it.
            open ? "is-open pointer-events-auto" : "pointer-events-none",
          )}
        >
          <p
            className={cn(
              "min-w-0 flex-1 break-words text-xs leading-relaxed",
              toast.kind === "error" ? "text-signal" : "text-ink/85",
            )}
          >
            {toast.message}
          </p>
          <Button
            size="icon"
            className="h-5 w-5 shrink-0"
            aria-label="Dismiss notification"
            onClick={() => dismiss(toast.id)}
          >
            <X size={12} />
          </Button>
        </div>
      ))}
    </div>
  );
}
