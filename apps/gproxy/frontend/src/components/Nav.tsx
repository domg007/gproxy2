export interface NavItem {
  id: string;
  label: string;
}

export function Nav({
  items,
  active,
  onChange
}: {
  items: NavItem[];
  active: string;
  onChange: (id: string) => void;
}) {
  return (
    <aside className="sidebar-shell">
      <nav className="space-y-1">
        {items.map((item) => (
          <button
            key={item.id}
            className={`nav-item ${active === item.id ? "nav-item-active" : ""}`}
            onClick={() => onChange(item.id)}
            type="button"
          >
            {item.label}
          </button>
        ))}
      </nav>
    </aside>
  );
}
