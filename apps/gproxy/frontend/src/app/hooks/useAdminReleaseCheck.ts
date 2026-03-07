import { useEffect, useRef } from "react";

import { apiRequest } from "../../lib/api";
import type { UserRole } from "../../lib/types";

const UPDATE_CHANNEL_STORAGE_KEY = "gproxy_update_channel";
const DEFAULT_UPDATE_CHANNEL = "releases";

type LatestReleaseResponse = {
  latest_release_tag?: string;
  current_version?: string;
  has_update?: boolean;
  update_channel?: string;
};

type NotifyFn = (kind: "success" | "error" | "info", message: string) => void;
type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

function normalizeUpdateChannel(value: string | null | undefined): string {
  const normalized = (value ?? "").trim().toLowerCase();
  return normalized === "staging" ? "staging" : DEFAULT_UPDATE_CHANNEL;
}

function readStoredUpdateChannel(): string {
  if (typeof window === "undefined") {
    return DEFAULT_UPDATE_CHANNEL;
  }
  return normalizeUpdateChannel(window.localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY));
}

export function useAdminReleaseCheck({
  apiKey,
  role,
  appVersion,
  notify,
  t
}: {
  apiKey: string | null;
  role: UserRole | null;
  appVersion: string;
  notify: NotifyFn;
  t: TranslateFn;
}) {
  const latestReleaseCheckKeyRef = useRef<string | null>(null);

  useEffect(() => {
    if (!apiKey || role !== "admin") {
      latestReleaseCheckKeyRef.current = null;
      return;
    }

    const checkKey = `${role}:${apiKey}`;
    if (latestReleaseCheckKeyRef.current === checkKey) {
      return;
    }
    latestReleaseCheckKeyRef.current = checkKey;

    let active = true;
    const run = async () => {
      try {
        const updateChannel = readStoredUpdateChannel();
        const result = await apiRequest<LatestReleaseResponse>(
          `/admin/system/latest_release?update_channel=${encodeURIComponent(updateChannel)}`,
          {
            apiKey,
            method: "GET"
          }
        );
        if (!active) {
          return;
        }
        if (result.has_update && result.latest_release_tag) {
          notify(
            "info",
            t("app.updateAvailable", {
              tag: result.latest_release_tag,
              current: result.current_version ?? appVersion,
              channel: result.update_channel ?? "release"
            })
          );
        }
      } catch {
        // Keep startup quiet if release check endpoint/network is unavailable.
      }
    };

    void run();
    return () => {
      active = false;
    };
  }, [apiKey, appVersion, notify, role, t]);
}
