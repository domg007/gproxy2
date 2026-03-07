import { useCallback, useEffect, useState } from "react";

import { apiRequest } from "../../lib/api";
import type { UserRole } from "../../lib/types";
import { detectRole } from "../session";

const API_KEY_STORAGE_KEY = "gproxy_api_key";
const ROLE_STORAGE_KEY = "gproxy_role";

type LoginResponse = {
  user_id: number;
  api_key: string;
};

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

export function useAppSession({
  t
}: {
  t: TranslateFn;
}) {
  const [apiKey, setApiKey] = useState<string | null>(null);
  const [role, setRole] = useState<UserRole | null>(null);
  const [loginLoading, setLoginLoading] = useState(false);
  const [restoringSession, setRestoringSession] = useState(true);

  useEffect(() => {
    let active = true;

    const restore = async () => {
      const storedApiKey = localStorage.getItem(API_KEY_STORAGE_KEY)?.trim();
      if (!storedApiKey) {
        setRestoringSession(false);
        return;
      }

      try {
        const session = await detectRole(storedApiKey);
        if (!active) {
          return;
        }
        setApiKey(storedApiKey);
        setRole(session.role);
        localStorage.setItem(ROLE_STORAGE_KEY, session.role);
      } catch {
        localStorage.removeItem(API_KEY_STORAGE_KEY);
        localStorage.removeItem(ROLE_STORAGE_KEY);
      } finally {
        if (active) {
          setRestoringSession(false);
        }
      }
    };

    void restore();
    return () => {
      active = false;
    };
  }, []);

  const login = useCallback(async (name: string, password: string) => {
    const userName = name.trim();
    if (!userName) {
      throw new Error(t("app.error.usernameEmpty"));
    }
    if (!password.trim()) {
      throw new Error(t("app.error.passwordEmpty"));
    }

    setLoginLoading(true);
    try {
      const loginResult = await apiRequest<LoginResponse>("/login", {
        method: "POST",
        body: { name: userName, password }
      });
      const nextApiKey = loginResult.api_key;
      const nextRole: UserRole = loginResult.user_id === 0 ? "admin" : "user";

      setApiKey(nextApiKey);
      setRole(nextRole);
      localStorage.setItem(API_KEY_STORAGE_KEY, nextApiKey);
      localStorage.setItem(ROLE_STORAGE_KEY, nextRole);

      return {
        apiKey: nextApiKey,
        role: nextRole
      };
    } finally {
      setLoginLoading(false);
    }
  }, [t]);

  const logout = useCallback(() => {
    localStorage.removeItem(API_KEY_STORAGE_KEY);
    localStorage.removeItem(ROLE_STORAGE_KEY);
    setApiKey(null);
    setRole(null);
    if (window.location.hash) {
      window.location.hash = "";
    }
  }, []);

  return {
    apiKey,
    role,
    loginLoading,
    restoringSession,
    login,
    logout
  };
}
