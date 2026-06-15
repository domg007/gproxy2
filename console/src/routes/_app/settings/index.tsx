import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";
import { instanceSettingsQuery, type InstanceSettings } from "@/api/settings";
import { SettingsForm } from "@/components/settings/settings-form";
import { UpdatePanel } from "@/components/update/update-panel";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export const Route = createFileRoute("/_app/settings/")({
  loader: ({ context }) => {
    void context.queryClient.ensureQueryData(instanceSettingsQuery);
  },
  component: SettingsPage,
});

function SettingsPage() {
  const { t } = useTranslation("settings");
  const { data: list = [], isPending } = useQuery(instanceSettingsQuery);
  const [selectedId, setSelectedId] = useState<number | null>(null);

  const selected: InstanceSettings | undefined =
    list.length === 0
      ? undefined
      : list.length === 1
        ? list[0]
        : (list.find((s) => s.id === selectedId) ?? list[0]);

  return (
    <div className="grid gap-4 p-4 md:p-6">
      <div>
        <h1 className="text-xl font-semibold">{t("title")}</h1>
        <p className="text-sm text-muted-foreground">{t("subtitle")}</p>
      </div>

      <Tabs defaultValue="general">
        <TabsList>
          <TabsTrigger value="general">{t("tabs.general")}</TabsTrigger>
          <TabsTrigger value="updates">{t("tabs.updates")}</TabsTrigger>
        </TabsList>

        <TabsContent value="general" className="grid gap-4 pt-4">
          {isPending ? (
            <div className="grid gap-4 md:grid-cols-2" aria-busy="true">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-40" />
              ))}
            </div>
          ) : (
            <>
              {list.length > 1 && (
                <div className="max-w-xs">
                  <Select
                    value={String(selected?.id ?? list[0].id)}
                    onValueChange={(v) => setSelectedId(Number(v))}
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {list.map((s) => (
                        <SelectItem key={s.id} value={String(s.id)}>
                          {s.instance_name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              )}

              {list.length === 0 && (
                <p className="text-sm text-muted-foreground">{t("empty")}</p>
              )}

              <SettingsForm key={selected?.id ?? "new"} settings={selected} />
            </>
          )}
        </TabsContent>

        <TabsContent value="updates" className="pt-4">
          <UpdatePanel />
        </TabsContent>
      </Tabs>
    </div>
  );
}
