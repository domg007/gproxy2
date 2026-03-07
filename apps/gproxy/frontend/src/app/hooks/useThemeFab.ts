import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import type { ThemeMode } from "../../lib/types";
import { applyTheme, persistTheme, readStoredTheme } from "../theme";

const THEME_FAB_POSITION_STORAGE_KEY = "gproxy_theme_fab_position";
const THEME_FAB_SIZE_PX = 48;
const THEME_FAB_MARGIN_PX = 12;

type ThemeFabPosition = {
  x: number;
  y: number;
};

type ThemeFabDragState = {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  dragged: boolean;
};

function defaultThemeFabPosition(): ThemeFabPosition {
  if (typeof window === "undefined") {
    return { x: THEME_FAB_MARGIN_PX, y: THEME_FAB_MARGIN_PX };
  }
  return {
    x: window.innerWidth - THEME_FAB_SIZE_PX - THEME_FAB_MARGIN_PX,
    y: window.innerHeight - THEME_FAB_SIZE_PX - THEME_FAB_MARGIN_PX
  };
}

function clampThemeFabPosition(position: ThemeFabPosition): ThemeFabPosition {
  if (typeof window === "undefined") {
    return position;
  }
  const maxX = Math.max(
    THEME_FAB_MARGIN_PX,
    window.innerWidth - THEME_FAB_SIZE_PX - THEME_FAB_MARGIN_PX
  );
  const maxY = Math.max(
    THEME_FAB_MARGIN_PX,
    window.innerHeight - THEME_FAB_SIZE_PX - THEME_FAB_MARGIN_PX
  );
  return {
    x: Math.min(Math.max(THEME_FAB_MARGIN_PX, position.x), maxX),
    y: Math.min(Math.max(THEME_FAB_MARGIN_PX, position.y), maxY)
  };
}

function readThemeFabPosition(): ThemeFabPosition {
  if (typeof window === "undefined") {
    return defaultThemeFabPosition();
  }
  try {
    const raw = localStorage.getItem(THEME_FAB_POSITION_STORAGE_KEY);
    if (!raw) {
      return defaultThemeFabPosition();
    }
    const parsed = JSON.parse(raw) as Partial<ThemeFabPosition>;
    const x = typeof parsed.x === "number" ? parsed.x : Number.NaN;
    const y = typeof parsed.y === "number" ? parsed.y : Number.NaN;
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
      return defaultThemeFabPosition();
    }
    return clampThemeFabPosition({ x, y });
  } catch {
    return defaultThemeFabPosition();
  }
}

function persistThemeFabPosition(position: ThemeFabPosition): void {
  if (typeof window === "undefined") {
    return;
  }
  localStorage.setItem(THEME_FAB_POSITION_STORAGE_KEY, JSON.stringify(position));
}

export function useThemeFab() {
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readStoredTheme());
  const [themeFabPosition, setThemeFabPosition] = useState<ThemeFabPosition>(() =>
    readThemeFabPosition()
  );
  const dragRef = useRef<ThemeFabDragState | null>(null);

  useEffect(() => {
    applyTheme(themeMode);
    persistTheme(themeMode);
  }, [themeMode]);

  useEffect(() => {
    persistThemeFabPosition(themeFabPosition);
  }, [themeFabPosition]);

  useEffect(() => {
    const onResize = () => {
      setThemeFabPosition((prev) => clampThemeFabPosition(prev));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    if (themeMode !== "system") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [themeMode]);

  const toggleTheme = () => {
    setThemeMode((prev) => (prev === "dark" ? "light" : "dark"));
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) {
      return;
    }
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: themeFabPosition.x,
      originY: themeFabPosition.y,
      dragged: false
    };
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const state = dragRef.current;
    if (!state || state.pointerId !== event.pointerId) {
      return;
    }

    const dx = event.clientX - state.startX;
    const dy = event.clientY - state.startY;
    if (!state.dragged && Math.abs(dx) + Math.abs(dy) >= 4) {
      state.dragged = true;
    }
    if (!state.dragged) {
      return;
    }

    setThemeFabPosition(
      clampThemeFabPosition({
        x: state.originX + dx,
        y: state.originY + dy
      })
    );
  };

  const finishPointer = (
    event: ReactPointerEvent<HTMLButtonElement>,
    cancelled: boolean
  ) => {
    const state = dragRef.current;
    if (!state || state.pointerId !== event.pointerId) {
      return;
    }
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = null;
    if (!cancelled && !state.dragged) {
      toggleTheme();
    }
  };

  return {
    themeMode,
    isDarkTheme: themeMode === "dark",
    themeFabPosition,
    onThemeFabPointerDown: onPointerDown,
    onThemeFabPointerMove: onPointerMove,
    onThemeFabPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) =>
      finishPointer(event, false),
    onThemeFabPointerCancel: (event: ReactPointerEvent<HTMLButtonElement>) =>
      finishPointer(event, true)
  };
}
