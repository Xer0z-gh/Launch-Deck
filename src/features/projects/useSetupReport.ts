/**
 * Launch readiness for one project.
 *
 * ONE query for the whole library, shared by every row: the answer is identical
 * for all of them, and a per-row query would fire fifty IPC calls on each
 * render pass to ask the same question fifty times.
 *
 * Lives in its own file because the row and its action cluster both need it,
 * and a component importing another component for a hook is how a cycle
 * starts.
 */
import { useQuery } from "@tanstack/react-query";

import { api, type SetupReportDto } from "@/lib/ipc";

export function useSetupReport(projectId: string): SetupReportDto | undefined {
  const { data } = useQuery({
    queryKey: ["setup-reports"],
    queryFn: () => api.setupReports(),
    // Dependencies do not appear on their own. The things that change this
    // answer -- an install, a relocate, a command edit -- all invalidate it
    // directly.
    staleTime: 60_000,
  });
  return data?.find((r) => r.projectId === projectId);
}
