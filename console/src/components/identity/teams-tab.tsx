import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Shield, Trash2, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { teamsQuery, upsertTeam, deleteTeam, type Org, type Team } from "@/api/identity";
import { ApiError } from "@/api/http";
import { ScopeAccessEditor } from "@/components/identity/scope-access-editor";
import { ConfirmDangerous } from "@/components/confirm-dangerous";
import { DataTable, type DataColumn } from "@/components/data-table";
import { EntityDialog } from "@/components/entity-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";

// ── Inline TeamForm ──────────────────────────────────────────────────────────

interface TeamFormProps {
  orgId: number;
  team?: Team;
  onSaved: () => void;
}

function TeamForm({ orgId, team, onSaved }: TeamFormProps) {
  const { t } = useTranslation("identity");
  const { t: tc } = useTranslation("common");
  const queryClient = useQueryClient();
  const editing = team !== undefined;

  const [name, setName] = useState(team?.name ?? "");
  const [enabled, setEnabled] = useState(team?.enabled ?? true);
  const [formError, setFormError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: () => {
      if (!name.trim()) throw new ApiError(0, "bad_request", t("teams.name") + " is required");
      return upsertTeam(orgId, {
        id: team?.id ?? null,
        org_id: orgId, // enforced = URL org_id
        name: name.trim(),
        enabled,
      });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["orgs", orgId, "teams"] });
      toast.success(tc("actions.save"));
      onSaved();
    },
    onError: (error) => {
      setFormError(error instanceof ApiError ? error.message : String(error));
    },
  });

  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        setFormError(null);
        mutation.mutate();
      }}
    >
      <div className="grid gap-2">
        <Label htmlFor="team-name">{t("teams.name")}</Label>
        <Input id="team-name" value={name} onChange={(e) => setName(e.target.value)} required />
      </div>
      <div className="flex items-center justify-between">
        <Label htmlFor="team-enabled">{t("teams.enabled")}</Label>
        <Switch id="team-enabled" checked={enabled} onCheckedChange={setEnabled} />
      </div>
      {formError && <p className="text-sm text-destructive">{formError}</p>}
      <Button type="submit" disabled={mutation.isPending}>
        {editing ? t("teams.edit") : t("teams.add")}
      </Button>
    </form>
  );
}

// ── TeamsTab ─────────────────────────────────────────────────────────────────

export function TeamsTab({ org }: { org: Org }) {
  const { t } = useTranslation("identity");
  const queryClient = useQueryClient();
  const { data: teams, isPending } = useQuery(teamsQuery(org.id));

  const [formOpen, setFormOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<Team | undefined>(undefined);
  const [deleteTarget, setDeleteTarget] = useState<Team | undefined>(undefined);
  const [accessTarget, setAccessTarget] = useState<Team | undefined>(undefined);

  const removal = useMutation({
    mutationFn: (id: number) => deleteTeam(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["orgs", org.id, "teams"] });
      setDeleteTarget(undefined);
    },
    onError: (error) => {
      toast.error(error instanceof ApiError ? error.message : String(error));
      setDeleteTarget(undefined);
    },
  });

  const openCreate = () => { setEditTarget(undefined); setFormOpen(true); };
  const openEdit = (team: Team) => { setEditTarget(team); setFormOpen(true); };

  const actions = (team: Team) => (
    <div className="flex items-center justify-end gap-1">
      <Button
        variant="ghost"
        size="icon"
        aria-label={t("teams.access")}
        onClick={(e) => { e.stopPropagation(); setAccessTarget(team); }}
      >
        <Shield className="size-4" aria-hidden />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        aria-label={t("teams.edit")}
        onClick={(e) => { e.stopPropagation(); openEdit(team); }}
      >
        <Pencil className="size-4" aria-hidden />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        className="text-destructive"
        aria-label={t("teams.delete")}
        onClick={(e) => { e.stopPropagation(); setDeleteTarget(team); }}
      >
        <Trash2 className="size-4" aria-hidden />
      </Button>
    </div>
  );

  const columns: DataColumn<Team>[] = [
    {
      key: "name",
      header: t("teams.name"),
      cell: (team) => <span className="font-medium">{team.name}</span>,
    },
    {
      key: "enabled",
      header: t("teams.enabled"),
      cell: (team) => (
        <Badge variant={team.enabled ? "secondary" : "outline"}>{team.enabled ? "on" : "off"}</Badge>
      ),
    },
    { key: "actions", header: "", cell: actions, className: "w-32 text-right" },
  ];

  return (
    <div className="grid gap-3">
      <div className="flex items-center justify-end">
        <Button onClick={openCreate}>
          <Plus className="size-4" aria-hidden />
          {t("teams.add")}
        </Button>
      </div>

      {isPending ? (
        <div className="grid gap-2" aria-busy="true">
          <Skeleton className="h-10" /><Skeleton className="h-10" />
        </div>
      ) : (
        <DataTable
          columns={columns}
          rows={teams ?? []}
          rowKey={(team) => team.id}
          empty={t("teams.empty")}
          renderCard={(team) => (
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <span className="font-medium">{team.name}</span>
                <Badge variant={team.enabled ? "secondary" : "outline"}>{team.enabled ? "on" : "off"}</Badge>
              </div>
              {actions(team)}
            </div>
          )}
        />
      )}

      <EntityDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        title={editTarget ? t("teams.edit") : t("teams.add")}
      >
        <TeamForm
          key={editTarget?.id ?? "new"}
          orgId={org.id}
          team={editTarget}
          onSaved={() => setFormOpen(false)}
        />
      </EntityDialog>

      <ConfirmDangerous
        open={deleteTarget !== undefined}
        onOpenChange={(o) => { if (!o) setDeleteTarget(undefined); }}
        title={t("teams.delete")}
        description={t("teams.deleteConfirm", { name: deleteTarget?.name ?? "" })}
        confirmLabel={t("teams.delete")}
        onConfirm={() => { if (deleteTarget) removal.mutate(deleteTarget.id); }}
        pending={removal.isPending}
      />

      <EntityDialog
        open={accessTarget !== undefined}
        onOpenChange={(o) => { if (!o) setAccessTarget(undefined); }}
        title={`${t("teams.access")} — ${accessTarget?.name ?? ""}`}
        wide
      >
        {accessTarget && (
          <ScopeAccessEditor scope="team" scopeId={accessTarget.id} />
        )}
      </EntityDialog>
    </div>
  );
}
