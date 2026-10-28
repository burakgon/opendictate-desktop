import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorText, useEvent, type ProviderInfo, type Settings, type SystemStatus } from "./api";

interface Store {
  settings: Settings;
  /** Mutates a draft copy and autosaves shortly after. */
  update: (fn: (draft: Settings) => void) => void;
  system: SystemStatus | null;
  refreshSystem: () => Promise<void>;
  providers: ProviderInfo[];
  saveError: string | null;
}

const Ctx = createContext<Store | null>(null);

export function StoreProvider({ children, fallback }: { children: ReactNode; fallback: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [saveError, setSaveError] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const latest = useRef<Settings | null>(null);

  const refreshSystem = useCallback(async () => {
    try {
      setSystem(await api.systemStatus());
    } catch (e) {
      console.error(e);
    }
  }, []);

  useEffect(() => {
    api.getSettings().then((s) => {
      latest.current = s;
      setSettings(s);
    });
    api.providers().then(setProviders);
    refreshSystem();
    const id = window.setInterval(refreshSystem, 5000);
    return () => window.clearInterval(id);
  }, [refreshSystem]);

  useEvent<Settings>("od://settings", (s) => {
    // Another window (or a reset) changed settings.
    if (timer.current === undefined) {
      latest.current = s;
      setSettings(s);
    }
  });

  const update = useCallback(
    (fn: (draft: Settings) => void) => {
      const base = latest.current;
      if (!base) return;
      const draft = structuredClone(base);
      fn(draft);
      latest.current = draft;
      setSettings(draft);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(async () => {
        timer.current = undefined;
        try {
          await api.saveSettings(latest.current!);
          setSaveError(null);
          refreshSystem();
        } catch (e) {
          setSaveError(errorText(e));
        }
      }, 350);
    },
    [refreshSystem],
  );

  if (!settings) return <>{fallback}</>;
  return <Ctx.Provider value={{ settings, update, system, refreshSystem, providers, saveError }}>{children}</Ctx.Provider>;
}

export function useStore(): Store {
  const s = useContext(Ctx);
  if (!s) throw new Error("useStore outside StoreProvider");
  return s;
}

export function providerName(providers: ProviderInfo[], id: string) {
  return providers.find((p) => p.id === id)?.name ?? id;
}

export function hasKey(settings: Settings, providers: ProviderInfo[], id: string): boolean {
  const info = providers.find((p) => p.id === id);
  if (!info) return false;
  if (!info.needs_key) return true;
  return (settings.providers[info.id]?.api_key ?? "").trim().length > 0;
}
