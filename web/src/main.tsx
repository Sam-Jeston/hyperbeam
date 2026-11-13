import { Activity, Bot, BookOpen, ChevronsUpDown, LayoutGrid, Loader2, LogOut, Menu as MenuIcon, Monitor, Moon, Plus, Settings, Sun, UserRound, Users } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "sonner";
import { Composer } from "./components/Composer";
import { BrandMark, Wordmark } from "./components/ui/brand";
import { Button } from "./components/ui/button";
import { ConfirmHost, Sheet } from "./components/ui/dialog";
import { Kbd } from "./components/ui/input";
import { Menu, MenuContent, MenuItem, MenuLabel, MenuRadio, MenuSeparator, MenuTrigger, Tooltip, TooltipProvider } from "./components/ui/menu";
import { isLive } from "./lib/api";
import { boot, signOut, useStore } from "./lib/store";
import { setTheme, type ThemeChoice, useTheme } from "./lib/theme";
import { A, Avatar, navigate, openComposer, Page, typing, useComposerOpen, useDocumentTitle, usePath } from "./lib/ui";
import { cn } from "./lib/utils";
import { AuthScreen } from "./pages/Auth";
import { ActivityPage, AgentsPage } from "./pages/Agents";
import { Board } from "./pages/Board";
import { LinkPage } from "./pages/Chat";
import { LibraryPage } from "./pages/Library";
import { SessionPage } from "./pages/Session";
import { SettingsPage } from "./pages/Settings";
import { TeamPage } from "./pages/Team";
import { WorkstreamPage } from "./pages/Workstream";

interface NavItem {
  href: string;
  label: string;
  icon: ReactNode;
  badge?: number;
  match: (path: string) => boolean;
}

function useNav(): { section?: string; items: NavItem[] }[] {
  const liveCount = useStore((s) => Object.values(s.sessions).filter(isLive).length);
  return [
    {
      items: [
        { href: "/", label: "Board", icon: <LayoutGrid />, match: (p) => p === "/" || p.startsWith("/w/") },
        { href: "/agents", label: "Agents", icon: <Bot />, badge: liveCount, match: (p) => p.startsWith("/agents") || p.startsWith("/s/") || p.startsWith("/sessions/") },
        { href: "/activity", label: "Activity", icon: <Activity />, match: (p) => p.startsWith("/activity") },
      ],
    },
    { section: "Build with", items: [{ href: "/library", label: "Library", icon: <BookOpen />, match: (p) => p.startsWith("/library") }] },
    {
      section: "Team",
      items: [
        { href: "/team", label: "People & permissions", icon: <Users />, match: (p) => p.startsWith("/team") },
        { href: "/settings", label: "Settings", icon: <Settings />, match: (p) => p.startsWith("/settings") },
      ],
    },
  ];
}

function NavList({ onNavigate }: { onNavigate?: () => void }) {
  const path = usePath();
  const groups = useNav();
  return (
    <nav className="grid gap-4" onClick={onNavigate}>
      {groups.map((group, i) => (
        <div key={i} className="grid gap-0.5">
          {group.section && <div className="px-2.5 pb-1 text-[11px] font-semibold uppercase tracking-[0.07em] text-faint">{group.section}</div>}
          {group.items.map((item) => {
            const active = item.match(path);
            return (
              <A
                key={item.href}
                href={item.href}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "group flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13.5px] font-medium transition-colors [&_svg]:size-4 [&_svg]:shrink-0",
                  active ? "bg-accent text-accent-foreground [&_svg]:text-primary" : "text-muted hover:bg-surface-2 hover:text-foreground [&_svg]:text-faint group-hover:[&_svg]:text-muted",
                )}
              >
                {item.icon}
                <span className="flex-1 truncate">{item.label}</span>
                {item.badge ? (
                  <Tooltip content={`${item.badge} agent${item.badge === 1 ? "" : "s"} live`}>
                    <span className="flex items-center gap-1 rounded-full bg-run/14 px-1.5 text-[11px] font-semibold tabular-nums text-run">
                      <span className="size-1.5 rounded-full bg-run animate-pulse-ring" />
                      {item.badge}
                    </span>
                  </Tooltip>
                ) : null}
              </A>
            );
          })}
        </div>
      ))}
    </nav>
  );
}

const THEMES: { value: ThemeChoice; label: string; icon: ReactNode }[] = [
  { value: "dark", label: "Dark", icon: <Moon /> },
  { value: "light", label: "Light", icon: <Sun /> },
  { value: "system", label: "Match system", icon: <Monitor /> },
];

function UserMenu() {
  const me = useStore((s) => s.me)!;
  const theme = useTheme();
  return (
    <Menu>
      <MenuTrigger asChild>
        <button className="flex w-full items-center gap-2.5 rounded-md p-1.5 text-left transition-colors hover:bg-surface-2 focus-visible:outline-2 focus-visible:outline-ring">
          <Avatar user={me} size={28} className="ring-0" />
          <span className="grid min-w-0 flex-1 leading-tight">
            <span className="truncate text-[13px] font-medium">{me.name}</span>
            <span className="truncate text-[12px] capitalize text-muted">{me.role}</span>
          </span>
          <ChevronsUpDown className="size-4 text-faint" />
        </button>
      </MenuTrigger>
      <MenuContent side="top" align="start" className="w-[var(--radix-dropdown-menu-trigger-width)] min-w-56">
        <MenuLabel>{me.email}</MenuLabel>
        <MenuItem onSelect={() => navigate("/settings/account")}><UserRound /> My account</MenuItem>
        <MenuSeparator />
        <MenuLabel>Theme</MenuLabel>
        <MenuRadio value={theme} onValueChange={(v) => setTheme(v as ThemeChoice)} options={THEMES} />
        <MenuSeparator />
        <MenuItem onSelect={() => void signOut()}><LogOut /> Sign out</MenuItem>
      </MenuContent>
    </Menu>
  );
}

function SidebarBody({ onNavigate }: { onNavigate?: () => void }) {
  const teamName = useStore((s) => s.meta?.team_name);
  return (
    <>
      <div className="flex items-center gap-2.5 px-2 pt-1 pb-4">
        <BrandMark size={26} />
        <div className="grid min-w-0 leading-tight">
          <Wordmark className="text-[17px]" />
          {teamName && <span className="truncate text-[12px] text-muted">{teamName}</span>}
        </div>
      </div>
      <Button
        variant="primary"
        className="mb-4 w-full justify-between"
        onClick={() => {
          onNavigate?.();
          openComposer();
        }}
      >
        <span className="flex items-center gap-1.5"><Plus /> New work</span>
        <Kbd className="border-primary-foreground/20 bg-primary-foreground/10 text-primary-foreground/80">C</Kbd>
      </Button>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <NavList onNavigate={onNavigate} />
      </div>
      <div className="mt-3 border-t border-border pt-3">
        <UserMenu />
      </div>
    </>
  );
}

function Shell() {
  const path = usePath();
  const connected = useStore((s) => s.connected);
  const composing = useComposerOpen();
  const [navOpen, setNavOpen] = useState(false);
  const teamName = useStore((s) => s.meta?.team_name);

  // C starts new work from anywhere, as long as you are not typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "c" && !e.metaKey && !e.ctrlKey && !e.altKey && !typing(e)) {
        e.preventDefault();
        openComposer();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  let page;
  const [, a, b] = path.split("/");
  if (a === "" || a === undefined) page = <Board />;
  else if (a === "w" && b) page = <WorkstreamPage key={b} id={b} />;
  else if ((a === "s" || a === "sessions") && b) page = <SessionPage key={b} id={b} />;
  else if (a === "agents") page = <AgentsPage />;
  else if (a === "activity") page = <ActivityPage />;
  else if (a === "library") page = <LibraryPage tab={b ?? "skills"} />;
  else if (a === "team") page = <TeamPage />;
  else if (a === "link" && b) page = <LinkPage key={b} code={b} />;
  else if (a === "settings") page = <SettingsPage tab={b ?? "integrations"} />;
  else page = <NotFound />;

  return (
    <div className="flex h-full">
      <aside className="hidden w-[244px] shrink-0 flex-col border-r border-border bg-surface/70 px-3 py-4 md:flex">
        <SidebarBody />
      </aside>
      <Sheet open={navOpen} onOpenChange={setNavOpen} title="Navigation">
        <div className="flex h-full flex-col px-3 py-4"><SidebarBody onNavigate={() => setNavOpen(false)} /></div>
      </Sheet>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border bg-surface/70 px-3 md:hidden">
          <Button variant="ghost" size="icon" aria-label="Open navigation" onClick={() => setNavOpen(true)}><MenuIcon /></Button>
          <BrandMark size={22} />
          <Wordmark className="text-[16px]" />
          {teamName && <span className="truncate text-[12.5px] text-muted">· {teamName}</span>}
          <Button variant="primary" size="icon-sm" className="ml-auto" aria-label="New work" onClick={() => openComposer()}><Plus /></Button>
        </div>
        {!connected && (
          <div role="status" className="flex items-center justify-center gap-2 border-b border-wait/25 bg-wait/10 px-4 py-1.5 text-[12.5px] text-wait">
            <Loader2 className="size-3.5 animate-spin" /> Reconnecting to the server. What you see may be out of date.
          </div>
        )}
        <main id="main-scroll" className="min-h-0 flex-1 overflow-y-auto">
          {page}
        </main>
      </div>
      {composing && <Composer onClose={() => openComposer(false)} />}
    </div>
  );
}

function NotFound() {
  useDocumentTitle("Not found");
  return (
    <Page width="narrow">
      <div className="grid place-items-center gap-3 py-24 text-center">
        <BrandMark size={44} className="opacity-60" />
        <h1 className="font-display text-2xl font-semibold">Nothing grows here</h1>
        <p className="text-muted">That page does not exist, or the link is out of date.</p>
        <Button variant="secondary" onClick={() => navigate("/")}>Back to the board</Button>
      </div>
    </Page>
  );
}

function Splash({ children }: { children?: ReactNode }) {
  return (
    <div className="grid h-full place-items-center p-6">
      <div className="grid justify-items-center gap-4 text-center">
        <BrandMark size={44} className={children ? undefined : "animate-shimmer"} />
        {children}
      </div>
    </div>
  );
}

function App() {
  const ready = useStore((s) => s.ready);
  const me = useStore((s) => s.me);
  const meta = useStore((s) => s.meta);
  const [bootError, setBootError] = useState<string | null>(null);

  useEffect(() => {
    boot().catch((e) => setBootError(e instanceof Error ? e.message : String(e)));
  }, []);

  if (bootError) {
    return (
      <Splash>
        <div className="grid gap-1">
          <h1 className="font-display text-xl font-semibold">Can’t reach Hyperbeam</h1>
          <p className="max-w-[46ch] text-muted">{bootError}</p>
        </div>
        <Button variant="primary" onClick={() => window.location.reload()}>Try again</Button>
      </Splash>
    );
  }
  if (!meta || (me && !ready)) return <Splash />;
  if (!me) return <AuthScreen />;
  return <Shell />;
}

function Root() {
  const theme = useTheme();
  return (
    <TooltipProvider delayDuration={350} skipDelayDuration={150}>
      <App />
      <ConfirmHost />
      <Toaster
        theme={theme === "system" ? "system" : theme}
        position="bottom-right"
        toastOptions={{
          classNames: {
            toast: "!rounded-lg !border !border-border !bg-elevated !text-foreground !shadow-pop !font-sans !text-[13.5px]",
            description: "!text-muted",
            success: "[&_[data-icon]]:!text-run",
            error: "[&_[data-icon]]:!text-fail",
          },
        }}
      />
    </TooltipProvider>
  );
}

createRoot(document.getElementById("root")!).render(<Root />);
