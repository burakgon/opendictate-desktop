import { AudioLines, History as HistoryIcon, Home as HomeIcon, KeyRound, Settings2, Wand2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { TitleBar } from "./components/TitleBar";
import { StatusDot, ToastProvider, cx } from "./components/ui";
import { api, useEvent, type Status } from "./lib/api";
import { StoreProvider, useStore } from "./lib/store";
import { Commands } from "./pages/Commands";
import { Dictation } from "./pages/Dictation";
import { History } from "./pages/History";
import { Home } from "./pages/Home";
import { Onboarding } from "./pages/Onboarding";
import { Providers } from "./pages/Providers";
import { SettingsPage } from "./pages/Settings";

type Page = "home" | "dictation" | "commands" | "providers" | "history" | "settings";

const NAV: { id: Page; label: string; icon: ReactNode }[] = [
  { id: "home", label: "Home", icon: <HomeIcon className="size-[18px]" /> },
  { id: "dictation", label: "Dictation", icon: <AudioLines className="size-[18px]" /> },
  { id: "commands", label: "Commands", icon: <Wand2 className="size-[18px]" /> },
  { id: "providers", label: "Providers", icon: <KeyRound className="size-[18px]" /> },
  { id: "history", label: "History", icon: <HistoryIcon className="size-[18px]" /> },
  { id: "settings", label: "Settings", icon: <Settings2 className="size-[18px]" /> },
];

function Shell() {
  const { settings, system, saveError } = useStore();
  const [page, setPage] = useState<Page>(() => {
    const h = window.location.hash.slice(1);
    return NAV.some((n) => n.id === h) ? (h as Page) : "home";
  });
  const [status, setStatus] = useState<Status | null>(null);
  const [onboarding, setOnboarding] = useState(!settings.onboarded);

  useEffect(() => {
    api.lastStatus().then((s) => s && setStatus(s));
  }, []);
  useEvent<Status>("od://status", setStatus);
  useEvent<string>("od://navigate", (p) => {
    if (NAV.some((n) => n.id === p)) setPage(p as Page);
  });

  if (onboarding) return <Onboarding onDone={() => setOnboarding(false)} />;

  const navigate = (p: string) => setPage(p as Page);
  const live = status?.phase === "recording" || status?.phase === "processing";
  const healthy = !!system && system.hotkeys.ok && system.injection_ok;

  return (
    <div className="app-frame flex h-full overflow-hidden rounded-[14px] border border-white/10">
      {/* Sidebar */}
      <aside className="flex w-[232px] shrink-0 flex-col border-r border-white/[0.06] bg-black/20">
        <div data-tauri-drag-region className="flex h-[72px] items-center gap-3 px-5">
          <img src="/logo.svg" alt="" className="pointer-events-none size-9" />
          <div className="pointer-events-none">
            <div className="text-[15.5px] font-semibold tracking-tight text-white">OpenDictate</div>
            <div className="text-[11.5px] text-white/35">Speak. It types.</div>
          </div>
        </div>
        <nav className="mt-2 flex-1 space-y-0.5 px-3">
          {NAV.map((n) => (
            <button
              key={n.id}
              onClick={() => setPage(n.id)}
              className={cx(
                "group relative flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-[14px] font-medium transition",
                page === n.id ? "bg-white/[0.08] text-white" : "text-white/50 hover:bg-white/[0.04] hover:text-white/85",
              )}
            >
              {page === n.id && <span className="absolute left-0 top-1/2 h-5 w-[3px] -translate-y-1/2 rounded-full brand-gradient" />}
              <span className={cx(page === n.id ? "text-brand-300" : "text-white/40 group-hover:text-white/70")}>{n.icon}</span>
              {n.label}
            </button>
          ))}
        </nav>
        <div className="m-3 rounded-xl border border-white/[0.06] bg-white/[0.03] px-3.5 py-3">
          <div className="flex items-center gap-2 text-[12.5px] font-medium text-white/75">
            {live ? (
              <>
                <span className="relative flex size-2.5">
                  <span className="absolute inset-0 animate-ping rounded-full bg-rose-400/70" />
                  <span className="relative size-2.5 rounded-full bg-rose-400" />
                </span>
                {status?.phase === "recording" ? "Listening…" : "Working…"}
              </>
            ) : (
              <>
                <StatusDot ok={healthy} warn={!!system && !healthy} />
                {healthy ? "Ready" : "Needs attention"}
              </>
            )}
          </div>
          <div className="mt-1 truncate text-[11.5px] text-white/35">
            {saveError ? <span className="text-rose-300">{saveError}</span> : `v${system?.version ?? ""} · ${system?.hotkeys.backend ?? ""} hotkeys`}
          </div>
        </div>
      </aside>

      {/* Content */}
      <main className="flex min-w-0 flex-1 flex-col">
        <TitleBar os={system?.environment.os} />
        <div className="scroll-area flex-1 px-10 pb-10 pt-1">
          <div key={page} className="mx-auto max-w-[980px] animate-fade-in">
            {page === "home" && <Home status={status} navigate={navigate} />}
            {page === "dictation" && <Dictation navigate={navigate} />}
            {page === "commands" && <Commands navigate={navigate} />}
            {page === "providers" && <Providers />}
            {page === "history" && <History />}
            {page === "settings" && <SettingsPage />}
          </div>
        </div>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <ToastProvider>
      <StoreProvider fallback={<div className="app-frame h-full rounded-[14px]" />}>
        <Shell />
      </StoreProvider>
    </ToastProvider>
  );
}
