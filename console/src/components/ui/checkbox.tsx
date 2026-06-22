import { useEffect, useRef } from "react";
import { cn } from "@/lib/utils";

interface CheckboxProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  indeterminate?: boolean;
  "aria-label"?: string;
  className?: string;
  disabled?: boolean;
}

/** 轻量复选框:原生 input,支持 indeterminate(表头"部分选中")。 */
export function Checkbox({ checked, onCheckedChange, indeterminate, className, disabled, ...rest }: CheckboxProps) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = Boolean(indeterminate) && !checked;
  }, [indeterminate, checked]);
  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      disabled={disabled}
      onChange={(e) => onCheckedChange(e.target.checked)}
      onClick={(e) => e.stopPropagation()}
      className={cn("size-4 accent-primary cursor-pointer", className)}
      aria-label={rest["aria-label"]}
    />
  );
}
