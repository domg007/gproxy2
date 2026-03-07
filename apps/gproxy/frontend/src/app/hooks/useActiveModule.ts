import { useCallback, useEffect, useState } from "react";

import type { UserRole } from "../../lib/types";

type ParseRoute = {
  role: UserRole;
  module: string;
};

function parseHashRoute(hash: string): ParseRoute | null {
  const trimmed = hash.startsWith("#") ? hash.slice(1) : hash;
  const segments = trimmed.split("/").filter(Boolean);
  if (segments.length < 2) {
    return null;
  }

  const [roleRaw, module] = segments;
  if ((roleRaw !== "admin" && roleRaw !== "user") || !module) {
    return null;
  }

  return { role: roleRaw, module };
}

function setHashRoute(role: UserRole, moduleId: string): void {
  const next = `#/${role}/${moduleId}`;
  if (window.location.hash !== next) {
    window.location.hash = next;
  }
}

export function useActiveModule({
  role,
  isValidModule,
  defaultModule
}: {
  role: UserRole | null;
  isValidModule: (role: UserRole, moduleId: string) => boolean;
  defaultModule: (role: UserRole) => string;
}) {
  const [activeModule, setActiveModule] = useState("");

  const syncModuleWithHash = useCallback((currentRole: UserRole) => {
    const parsed = parseHashRoute(window.location.hash);
    if (parsed && parsed.role === currentRole && isValidModule(currentRole, parsed.module)) {
      setActiveModule(parsed.module);
      return;
    }

    const fallback = defaultModule(currentRole);
    setActiveModule(fallback);
    setHashRoute(currentRole, fallback);
  }, [defaultModule, isValidModule]);

  useEffect(() => {
    if (!role) {
      return;
    }

    const onHashChange = () => syncModuleWithHash(role);
    onHashChange();
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [role, syncModuleWithHash]);

  const selectModule = useCallback((currentRole: UserRole, moduleId: string) => {
    setActiveModule(moduleId);
    setHashRoute(currentRole, moduleId);
  }, []);

  const initializeModule = useCallback((currentRole: UserRole) => {
    const fallback = defaultModule(currentRole);
    setActiveModule(fallback);
    setHashRoute(currentRole, fallback);
  }, [defaultModule]);

  const resetModule = useCallback(() => {
    setActiveModule("");
  }, []);

  return {
    activeModule,
    selectModule,
    initializeModule,
    resetModule
  };
}
