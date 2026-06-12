import { Outlet, createFileRoute, redirect } from "@tanstack/react-router";
import { sessionQuery } from "@/api/auth";

export const Route = createFileRoute("/_app")({
  beforeLoad: async ({ context }) => {
    try {
      const user = await context.queryClient.ensureQueryData(sessionQuery);
      return { user };
    } catch {
      throw redirect({ to: "/login" });
    }
  },
  component: AppLayout,
});

function AppLayout() {
  // Task 7 wraps this in <AppShell>.
  return <Outlet />;
}
