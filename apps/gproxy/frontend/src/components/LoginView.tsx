import { useState } from "react";

import { useI18n } from "../app/i18n";
import { Button, Card, Input, Label } from "./ui";

export function LoginView({
  onLogin,
  loading
}: {
  onLogin: (key: string) => Promise<void>;
  loading: boolean;
}) {
  const { t } = useI18n();
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState("");

  return (
    <div className="mx-auto mt-20 w-full max-w-lg px-4">
      <Card title={t("login.title")} subtitle={t("login.subtitle")}>
        <form
          className="space-y-3"
          onSubmit={async (event) => {
            event.preventDefault();
            setError("");
            try {
              await onLogin(apiKey.trim());
            } catch (err) {
              setError(err instanceof Error ? err.message : String(err));
            }
          }}
        >
          <div>
            <Label>{t("login.apiKey")}</Label>
            <Input
              type="password"
              value={apiKey}
              onChange={setApiKey}
              placeholder={t("login.placeholder")}
            />
          </div>
          {error ? <p className="text-sm text-red-500">{error}</p> : null}
          <Button type="submit" disabled={loading}>
            {loading ? t("login.submitting") : t("login.submit")}
          </Button>
        </form>
      </Card>
    </div>
  );
}
