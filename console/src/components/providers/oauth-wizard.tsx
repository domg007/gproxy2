import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  cookieLogin, deviceStart, devicePoll, loginFlowComplete, loginFlowStart,
  type DeviceStartResponse, type LoginStartResponse,
} from "@/api/login-flows";
import type { CredentialView } from "@/api/credentials";
import { channelMeta, type LoginMode } from "@/lib/channel-meta";
import { extractSessionKey, validateCallbackUrl } from "@/lib/oauth-input";
import type { Provider } from "@/api/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

interface OAuthWizardProps {
  provider: Provider;
  onDone: (credential: CredentialView) => void;
}

const KIRO_PROVIDERS = ["Enterprise", "BuilderId", "Internal"] as const;

export function OAuthWizard({ provider, onDone }: OAuthWizardProps) {
  const { t } = useTranslation("providers");
  const queryClient = useQueryClient();
  const meta = channelMeta(provider.channel);
  const modes = meta?.loginModes ?? [];
  const [mode, setMode] = useState<LoginMode>(modes[0] ?? "authcode");
  const [credLabel, setCredLabel] = useState("");

  const finish = (credential: CredentialView) => {
    void queryClient.invalidateQueries({ queryKey: ["providers", provider.id, "credentials"] });
    onDone(credential);
  };

  return (
    <div className="grid gap-4">
      {modes.length > 1 && (
        <Tabs value={mode} onValueChange={(v) => setMode(v as LoginMode)}>
          <TabsList>
            {modes.map((m) => (
              <TabsTrigger key={m} value={m}>{t(`wizard.mode.${m}`)}</TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      )}
      <div className="grid gap-2">
        <Label htmlFor="w-name">{t("wizard.credName")}</Label>
        <Input id="w-name" value={credLabel} onChange={(e) => setCredLabel(e.target.value)} />
      </div>
      {mode === "authcode" && <AuthcodeFlow provider={provider} credLabel={credLabel} onDone={finish} />}
      {mode === "device" && <DeviceFlow provider={provider} credLabel={credLabel} onDone={finish} />}
      {mode === "cookie" && <CookieFlow provider={provider} credLabel={credLabel} onDone={finish} />}
    </div>
  );
}

interface FlowProps {
  provider: Provider;
  credLabel: string;
  onDone: (credential: CredentialView) => void;
}

function AuthcodeFlow({ provider, credLabel, onDone }: FlowProps) {
  const { t } = useTranslation("providers");
  const [session, setSession] = useState<LoginStartResponse | null>(null);
  const [pasted, setPasted] = useState("");
  const [pasteTouched, setPasteTouched] = useState(false);
  // Kiro dual-mode params
  const isKiro = provider.channel === "kiro";
  const [kiroIdc, setKiroIdc] = useState(false);
  const [kiroProvider, setKiroProvider] = useState<string>("Enterprise");
  const [kiroStartUrl, setKiroStartUrl] = useState("");
  const [kiroRegion, setKiroRegion] = useState("us-east-1");

  const start = useMutation({
    mutationFn: () => {
      const params = isKiro && kiroIdc
        ? {
            auth_method: "idc",
            provider: kiroProvider,
            region: kiroRegion.trim() || "us-east-1",
            ...(kiroStartUrl.trim() !== "" ? { start_url: kiroStartUrl.trim() } : {}),
          }
        : undefined;
      return loginFlowStart({ channel: provider.channel, params });
    },
    onSuccess: (resp) => {
      setSession(resp);
      window.open(resp.authorize_url, "_blank", "noopener");
    },
  });

  const complete = useMutation({
    mutationFn: () => {
      if (session === null) return Promise.reject(new Error("no session"));
      return loginFlowComplete({
        login_session_id: session.login_session_id,
        callback_url: pasted.trim(),
        provider_id: provider.id,
        ...(credLabel.trim() !== "" ? { name: credLabel.trim() } : {}),
      });
    },
    onSuccess: onDone,
  });

  const pasteValid = session !== null && validateCallbackUrl(pasted, session.authorize_url);
  const kiroStartUrlMissing = isKiro && kiroIdc && kiroProvider === "Enterprise" && kiroStartUrl.trim() === "";

  if (session === null) {
    return (
      <div className="grid gap-4">
        {isKiro && (
          <>
            <div className="grid gap-2">
              <Label>{t("wizard.kiroMode")}</Label>
              <Tabs value={kiroIdc ? "idc" : "social"} onValueChange={(v) => setKiroIdc(v === "idc")}>
                <TabsList>
                  <TabsTrigger value="social">{t("wizard.kiroSocial")}</TabsTrigger>
                  <TabsTrigger value="idc">{t("wizard.kiroIdc")}</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>
            {kiroIdc && (
              <div className="grid gap-4">
                <div className="grid gap-2">
                  <Label>{t("wizard.kiroProvider")}</Label>
                  <Select value={kiroProvider} onValueChange={setKiroProvider}>
                    <SelectTrigger><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {KIRO_PROVIDERS.map((p) => <SelectItem key={p} value={p}>{p}</SelectItem>)}
                    </SelectContent>
                  </Select>
                </div>
                <div className="grid gap-2">
                  <Label htmlFor="kiro-start">{t("wizard.kiroStartUrl")}</Label>
                  <Input id="kiro-start" value={kiroStartUrl} onChange={(e) => setKiroStartUrl(e.target.value)}
                    placeholder="https://….awsapps.com/start" />
                  <p className="text-xs text-muted-foreground">{t("wizard.kiroStartUrlHint")}</p>
                </div>
                <div className="grid gap-2">
                  <Label htmlFor="kiro-region">{t("wizard.kiroRegion")}</Label>
                  <Input id="kiro-region" value={kiroRegion} onChange={(e) => setKiroRegion(e.target.value)} />
                </div>
              </div>
            )}
          </>
        )}
        {start.isError && <p className="text-sm text-destructive">{t("wizard.failed")}</p>}
        <Button onClick={() => start.mutate()} disabled={start.isPending || kiroStartUrlMissing}>
          {start.isPending ? t("wizard.starting") : t("wizard.start")}
        </Button>
      </div>
    );
  }

  return (
    <div className="grid gap-4">
      <Button variant="outline" onClick={() => window.open(session.authorize_url, "_blank", "noopener")}>
        <ExternalLink className="size-4" />
        {t("wizard.openAuthorize")}
      </Button>
      <div className="grid gap-2">
        <Label htmlFor="w-cb">{t("wizard.pasteLabel")}</Label>
        <Textarea id="w-cb" rows={3} value={pasted} spellCheck={false}
          onChange={(e) => setPasted(e.target.value)} onBlur={() => setPasteTouched(true)} />
        <p className={pasteTouched && pasted.trim() !== "" && !pasteValid ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
          {pasteTouched && pasted.trim() !== "" && !pasteValid ? t("wizard.pasteInvalid") : t("wizard.pasteHint")}
        </p>
      </div>
      {complete.isError && <p className="text-sm text-destructive">{t("wizard.failed")}</p>}
      <Button onClick={() => complete.mutate()} disabled={!pasteValid || complete.isPending}>
        {complete.isPending ? t("wizard.completing") : t("wizard.complete")}
      </Button>
    </div>
  );
}

function DeviceFlow({ provider, credLabel, onDone }: FlowProps) {
  const { t } = useTranslation("providers");
  const [session, setSession] = useState<DeviceStartResponse | null>(null);
  const [failed, setFailed] = useState(false);
  const stopped = useRef(false);
  // Keep the latest onDone without retriggering the poll effect (parent re-renders
  // recreate the closure; resetting the timer on each render would stall polling).
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  const start = useMutation({
    mutationFn: () =>
      deviceStart({
        channel: provider.channel,
        provider_id: provider.id,
        ...(credLabel.trim() !== "" ? { name: credLabel.trim() } : {}),
      }),
    onSuccess: (resp) => { setFailed(false); setSession(resp); },
  });

  useEffect(() => {
    if (session === null) return;
    stopped.current = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const resp = await devicePoll(session.login_session_id);
        if (stopped.current) return;
        if (resp.status === "ready") {
          onDoneRef.current(resp.credential);
          return;
        }
        timer = setTimeout(() => void tick(), Math.max(session.interval_secs, 2) * 1000);
      } catch {
        if (!stopped.current) { setFailed(true); setSession(null); }
      }
    };
    timer = setTimeout(() => void tick(), Math.max(session.interval_secs, 2) * 1000);
    return () => { stopped.current = true; clearTimeout(timer); };
  }, [session]);

  if (session === null) {
    return (
      <div className="grid gap-4">
        {(failed || start.isError) && <p className="text-sm text-destructive">{t("wizard.failed")}</p>}
        <Button onClick={() => start.mutate()} disabled={start.isPending}>
          {start.isPending ? t("wizard.starting") : t("wizard.start")}
        </Button>
      </div>
    );
  }

  return (
    <div className="grid gap-3 text-center">
      <p className="text-sm text-muted-foreground">{t("wizard.deviceIntro")}</p>
      <a className="text-sm font-medium underline" href={session.verification_url} target="_blank" rel="noopener noreferrer">
        {session.verification_url}
      </a>
      <p className="font-mono text-2xl font-semibold tracking-widest">{session.user_code}</p>
      <p className="text-xs text-muted-foreground">{t("wizard.waiting")}</p>
    </div>
  );
}

function CookieFlow({ provider, credLabel, onDone }: FlowProps) {
  const { t } = useTranslation("providers");
  const [pasted, setPasted] = useState("");
  const [touched, setTouched] = useState(false);
  const cookie = extractSessionKey(pasted);

  const mutation = useMutation({
    mutationFn: () => {
      if (cookie === null) return Promise.reject(new Error("no cookie"));
      return cookieLogin({
        channel: provider.channel,
        cookie,
        provider_id: provider.id,
        ...(credLabel.trim() !== "" ? { name: credLabel.trim() } : {}),
      });
    },
    onSuccess: onDone,
  });

  return (
    <div className="grid gap-4">
      <div className="grid gap-2">
        <Label htmlFor="w-cookie">{t("wizard.cookieLabel")}</Label>
        <Textarea id="w-cookie" rows={3} value={pasted} spellCheck={false} autoComplete="off"
          onChange={(e) => setPasted(e.target.value)} onBlur={() => setTouched(true)} />
        <p className={touched && pasted.trim() !== "" && cookie === null ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
          {touched && pasted.trim() !== "" && cookie === null ? t("wizard.cookieInvalid") : t("wizard.cookieHint")}
        </p>
      </div>
      {mutation.isError && <p className="text-sm text-destructive">{t("wizard.failed")}</p>}
      <Button onClick={() => mutation.mutate()} disabled={cookie === null || mutation.isPending}>
        {mutation.isPending ? t("wizard.completing") : t("wizard.complete")}
      </Button>
    </div>
  );
}
