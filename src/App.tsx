import { Mic, Settings, X } from "lucide-react";
import { useState } from "react";
import { RecordingIndicator } from "@/components/RecordingIndicator";
import { SettingsPanel } from "@/components/SettingsPanel";
import { StatusBanner } from "@/components/StatusBanner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { AsrProvider, useAsrClient } from "@/hooks/useAsrClient";
import { cn } from "@/lib/utils";

type Tab = "dictate" | "settings";

function Header({ tab, onTab }: { tab: Tab; onTab: (t: Tab) => void }) {
  const { status, closePanel } = useAsrClient();
  const online = status?.health === "online";
  return (
    <header className="flex items-center justify-between px-4 pt-4 pb-2">
      <div className="flex items-center gap-2">
        <Mic className="size-4" />
        <h1 className="text-sm font-semibold">ASR Dictation</h1>
        <Badge variant={online ? "secondary" : "destructive"}>
          {online ? "online" : "offline"}
        </Badge>
      </div>
      <div className="flex items-center gap-1">
        <Button
          variant={tab === "dictate" ? "secondary" : "ghost"}
          size="sm"
          onClick={() => onTab("dictate")}
        >
          Dictate
        </Button>
        <Button
          variant={tab === "settings" ? "secondary" : "ghost"}
          size="sm"
          aria-label="Settings"
          onClick={() => onTab("settings")}
        >
          <Settings />
        </Button>
        <Button variant="ghost" size="sm" aria-label="Close" onClick={closePanel}>
          <X />
        </Button>
      </div>
    </header>
  );
}

function Panel() {
  const [tab, setTab] = useState<Tab>("dictate");
  return (
    <div className="flex h-full flex-col">
      <Header tab={tab} onTab={setTab} />
      <main
        className={cn(
          "flex-1 overflow-y-auto px-4 pb-4",
          tab === "dictate" && "flex flex-col justify-center gap-6",
        )}
      >
        {tab === "dictate" ? (
          <>
            <RecordingIndicator />
            <StatusBanner />
          </>
        ) : (
          <SettingsPanel />
        )}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <AsrProvider>
      <Panel />
    </AsrProvider>
  );
}
