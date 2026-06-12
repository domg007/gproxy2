import { createFileRoute, redirect } from "@tanstack/react-router";
import { sessionQuery } from "@/api/auth";

export const Route = createFileRoute("/login")({
  beforeLoad: async ({ context }) => {
    try {
      await context.queryClient.ensureQueryData(sessionQuery);
    } catch {
      return; // not signed in — show the login page
    }
    throw redirect({ to: "/" });
  },
  component: LoginPage,
});

function LoginPage() {
  return <div className="p-8">login form — Task 6</div>;
}
