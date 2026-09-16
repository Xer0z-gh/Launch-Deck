import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRoot } from "react-dom/client";

import App from "@/App";
import { asIpcError } from "@/lib/ipc";
import { initRuntimeEvents } from "@/state/runtime";
import { applyTheme, initTheme, useUi } from "@/state/ui";

import "@/styles/tokens.css";

// The first line of our own code to run. Everything before it -- process
// start, WebView2 creation, HTML parse, module fetch and JS parse -- is
// bundled cost we can only reduce by shipping less.
performance.mark("script-eval");

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A local IPC call is not a flaky network, so most failures are real and
      // must surface immediately. The exception is a briefly-locked SQLite
      // file, which the backend already marks `retryable` -- retrying those a
      // couple of times turns a momentary contention into a hiccup instead of
      // an error screen the user has to dismiss.
      retry: (attempt, error) => attempt < 2 && asIpcError(error).retryable,
      retryDelay: (attempt) => 250 * (attempt + 1),
      refetchOnWindowFocus: false,
    },
  },
});

// Paint with the cached theme immediately; the DB-persisted one follows.
applyTheme(useUi.getState().theme);
void initTheme();

// Event listeners are process-level singletons, registered before React mounts
// so component lifecycles (and StrictMode double-effects) cannot duplicate them.
void initRuntimeEvents(() => {
  void queryClient.invalidateQueries({ queryKey: ["projects"] });
});

// DEV-only hook for headless E2E drivers and debugging: lets an external
// harness trigger the same invalidation the in-app mutations perform. Never
// present in release builds.
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>)["__deck"] = {
    invalidateProjects: () =>
      queryClient.invalidateQueries({ queryKey: ["projects"] }),
  };
}

const container = document.getElementById("root");
if (container === null) {
  throw new Error("index.html is missing #root");
}

performance.mark("react-mount");
createRoot(container).render(
  // No motion provider: every transition in this app is CSS, in `tokens.css`,
  // driven by `lib/presence.ts`. `prefers-reduced-motion` is honoured by the
  // media query at the end of that stylesheet, which applies to the whole
  // document rather than only to components that remembered to opt in.
  <QueryClientProvider client={queryClient}>
    <App />
  </QueryClientProvider>,
);
