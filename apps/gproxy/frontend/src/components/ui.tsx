import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEventHandler,
  type MouseEventHandler,
  type ReactNode
} from "react";

export function Card({
  title,
  subtitle,
  action,
  children
}: {
  title?: string;
  subtitle?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="card-shell">
      {title || subtitle || action ? (
        <header className="mb-4 flex flex-wrap items-start justify-between gap-3">
          <div>
            {title ? <h2 className="text-lg font-semibold text-text">{title}</h2> : null}
            {subtitle ? <p className="mt-1 text-sm text-muted">{subtitle}</p> : null}
          </div>
          {action}
        </header>
      ) : null}
      {children}
    </section>
  );
}

export function Button({
  children,
  onClick,
  variant = "primary",
  type = "button",
  disabled
}: {
  children: ReactNode;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  variant?: "primary" | "neutral" | "danger" | "secondary";
  type?: "button" | "submit";
  disabled?: boolean;
}) {
  const normalized = variant === "secondary" ? "neutral" : variant;
  return (
    <button className={`btn btn-${normalized}`} onClick={onClick} type={type} disabled={disabled}>
      {children}
    </button>
  );
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <label className="mb-1 block text-xs font-semibold uppercase tracking-[0.1em] text-muted">
      {children}
    </label>
  );
}

export function Input({
  value,
  onChange,
  placeholder,
  type = "text",
  disabled,
  readOnly
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: "text" | "number" | "password" | "datetime-local";
  disabled?: boolean;
  readOnly?: boolean;
}) {
  return (
    <input
      className="input"
      value={value}
      type={type}
      disabled={disabled}
      readOnly={readOnly}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

export function TextArea({
  value,
  onChange,
  rows = 5,
  placeholder,
  readOnly
}: {
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  placeholder?: string;
  readOnly?: boolean;
}) {
  return (
    <textarea
      className="textarea"
      value={value}
      rows={rows}
      readOnly={readOnly}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

export function Select({
  value,
  onChange,
  options,
  disabled
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
}) {
  return (
    <select
      className="select"
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {options.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  );
}

export function SearchableSelect({
  value,
  onChange,
  options,
  placeholder,
  disabled,
  noResultLabel = "No matches"
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
  placeholder?: string;
  disabled?: boolean;
  noResultLabel?: string;
}) {
  const blurTimer = useRef<number | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);

  const selectedOption = useMemo(
    () => options.find((item) => item.value === value) ?? null,
    [options, value]
  );

  useEffect(() => {
    setQuery(selectedOption && selectedOption.value !== "" ? selectedOption.label : "");
  }, [selectedOption]);

  useEffect(
    () => () => {
      if (blurTimer.current !== null) {
        window.clearTimeout(blurTimer.current);
      }
    },
    []
  );

  const filteredOptions = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return options;
    }
    return options.filter((item) => item.label.toLowerCase().includes(needle));
  }, [options, query]);

  const commit = (next: { value: string; label: string }) => {
    onChange(next.value);
    setQuery(next.value === "" ? "" : next.label);
    setOpen(false);
  };

  const handleInputChange = (text: string) => {
    setQuery(text);
    setOpen(true);
    if (!text.trim()) {
      onChange("");
    } else if (selectedOption && text !== selectedOption.label) {
      onChange("");
    }
  };

  const handleBlur = () => {
    blurTimer.current = window.setTimeout(() => {
      setOpen(false);
    }, 120);
  };

  const handleKeyDown: KeyboardEventHandler<HTMLInputElement> = (event) => {
    if (event.key === "Escape") {
      setOpen(false);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const firstMatch = filteredOptions[0];
      if (firstMatch) {
        commit(firstMatch);
      }
    }
  };

  return (
    <div className="search-select">
      <input
        className="input"
        value={query}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(event) => handleInputChange(event.target.value)}
        onFocus={() => setOpen(true)}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
      />
      {open && !disabled ? (
        <div className="search-select-list">
          {filteredOptions.length > 0 ? (
            filteredOptions.map((item) => (
              <button
                key={item.value || "__all__"}
                type="button"
                className="search-select-item"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => commit(item)}
              >
                {item.label}
              </button>
            ))
          ) : (
            <div className="search-select-empty">{noResultLabel}</div>
          )}
        </div>
      ) : null}
    </div>
  );
}

export function Table({
  columns,
  rows
}: {
  columns: string[];
  rows: Array<Record<string, ReactNode>>;
}) {
  return (
    <div className="data-table-wrap">
      <table className="data-table">
        <thead>
          <tr>
            {columns.map((column) => (
              <th key={column}>{column}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={index}>
              {columns.map((column) => (
                <td key={column}>{row[column]}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function MetricCard({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="metric-card">
      <div className="text-xs uppercase tracking-[0.08em] text-muted">{label}</div>
      <div className="mt-2 text-2xl font-semibold text-text">{value}</div>
    </div>
  );
}

export function Badge({
  children,
  active
}: {
  children: ReactNode;
  active?: boolean;
}) {
  return <span className={`badge ${active ? "badge-active" : ""}`}>{children}</span>;
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel,
  confirmVariant = "danger",
  busy = false,
  onConfirm,
  onClose
}: {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  confirmVariant?: "primary" | "neutral" | "danger" | "secondary";
  busy?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const titleId = useId();

  useEffect(() => {
    if (!open) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [open, onClose]);

  if (!open) {
    return null;
  }

  return (
    <div className="dialog-backdrop" role="presentation" onClick={busy ? undefined : onClose}>
      <div
        className="dialog-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="dialog-header">
          <h3 id={titleId} className="dialog-title">
            {title}
          </h3>
        </div>
        <p className="dialog-description">{description}</p>
        <div className="dialog-actions">
          <Button variant="neutral" onClick={onClose} disabled={busy}>
            {cancelLabel}
          </Button>
          <Button variant={confirmVariant} onClick={onConfirm} disabled={busy}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
