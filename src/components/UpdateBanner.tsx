import { openUrl } from "@tauri-apps/plugin-opener";
import { Download, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errorText, useEvent, type UpdateInfo, type UpdateProgress } from "../lib/api";
import { Button, useToast } from "./ui";

/** "OpenDictate X is available", with a one-click install where this copy can update itself. */
export function UpdateBanner() {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const toast = useToast();
  useEffect(() => {
    api.updateInfo().then(setUpdate);
  }, []);
  useEvent<UpdateInfo>("od://update", setUpdate);
  useEvent<UpdateProgress>("od://update-progress", setProgress);
  if (!update) return null;

  const percent = progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;
  const install = async () => {
    setInstalling(true);
    try {
      await api.installUpdate(); // OpenDictate restarts when this succeeds
    } catch (e) {
      toast(errorText(e), "error");
      setInstalling(false);
      setProgress(null);
    }
  };

  return (
    <div className="mb-5 flex items-center gap-3 rounded-xl border border-brand-400/25 bg-brand-500/[0.08] px-4 py-3">
      <Sparkles className="size-4 shrink-0 text-brand-300" />
      <div className="min-w-0 flex-1 text-[13.5px] text-white/85">
        {installing ? (
          <>
            {percent === null ? "Downloading OpenDictate " + update.version + "…" : `Downloading OpenDictate ${update.version}… ${percent}%`}
            <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-white/10">
              <div className="h-full rounded-full brand-gradient transition-[width] duration-300" style={{ width: `${percent ?? 8}%` }} />
            </div>
          </>
        ) : (
          <>OpenDictate {update.version} is available.</>
        )}
      </div>
      <Button size="sm" variant="ghost" onClick={() => openUrl(update.url)}>
        What's new
      </Button>
      {update.can_install && (
        <Button size="sm" variant="primary" icon={<Download className="size-3.5" />} loading={installing} onClick={install}>
          Update &amp; restart
        </Button>
      )}
    </div>
  );
}
