import { useState, type ReactNode } from "react";
import { Menu } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LocaleControls } from "@/components/locale-controls";
import { NavList } from "@/components/shell/nav";
import { UserMenu } from "@/components/shell/user-menu";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";

function Brand({ compact }: { compact?: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-14 items-center gap-2 px-4 font-semibold">
      <span className="grid size-7 shrink-0 place-items-center rounded-md bg-primary text-xs text-primary-foreground">
        g
      </span>
      <span className={compact ? "hidden xl:inline" : undefined}>{t("app.name")}</span>
    </div>
  );
}

export function AppShell({ children }: { children: ReactNode }) {
  const [drawerOpen, setDrawerOpen] = useState(false);
  const { t } = useTranslation();
  return (
    <div className="flex min-h-svh">
      {/* tablet: icon rail (md..xl) · desktop: full sidebar (xl+) */}
      <aside className="sticky top-0 hidden h-svh w-14 shrink-0 flex-col border-r bg-background md:flex xl:w-60">
        <Brand compact />
        <div className="flex-1 overflow-y-auto py-2">
          <NavList compact />
        </div>
        <div className="border-t p-2 text-center text-[10px] text-muted-foreground xl:text-left xl:px-4">
          <span className="hidden xl:inline">v{__APP_VERSION__} · {__APP_COMMIT__}</span>
          <span className="xl:hidden">v{__APP_VERSION__}</span>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        {/* top bar: mobile gets the drawer trigger; all sizes get controls */}
        <header className="sticky top-0 z-10 flex h-14 items-center gap-2 border-b bg-background/95 px-4 backdrop-blur">
          <Sheet open={drawerOpen} onOpenChange={setDrawerOpen}>
            <SheetTrigger asChild>
              <Button variant="ghost" size="icon" className="md:hidden" aria-label={t("nav.openMenu")}>
                <Menu className="size-5" />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-64 p-0">
              <SheetHeader className="p-0">
                <SheetTitle className="sr-only">{t("nav.title")}</SheetTitle>
              </SheetHeader>
              <Brand />
              <div className="py-2">
                <NavList onNavigate={() => setDrawerOpen(false)} />
              </div>
            </SheetContent>
          </Sheet>
          <div className="flex-1" />
          <LocaleControls />
          <UserMenu />
        </header>
        <main className="min-w-0 flex-1">{children}</main>
      </div>
    </div>
  );
}
