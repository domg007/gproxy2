import { useCallback, useEffect, useRef, useState } from "react";

import type { ToastState } from "../../components/Toast";

export function useTimedToast(durationMs = 2600) {
  const [toast, setToast] = useState<ToastState | null>(null);
  const timerRef = useRef<number | null>(null);

  const notify = useCallback((kind: ToastState["kind"], message: string) => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }

    setToast({ kind, message });
    timerRef.current = window.setTimeout(() => {
      setToast(null);
      timerRef.current = null;
    }, durationMs);
  }, [durationMs]);

  useEffect(
    () => () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
      }
    },
    []
  );

  return {
    toast,
    notify
  };
}
