import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/_portal/account/keys/")({
  component: KeysStub,
});

function KeysStub() {
  return <div className="p-8 text-muted-foreground">API Keys — coming soon (Task 3).</div>;
}
