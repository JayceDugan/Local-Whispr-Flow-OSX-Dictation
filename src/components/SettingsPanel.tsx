import { KeyRound, TriangleAlert } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { useAsrClient } from "@/hooks/useAsrClient";

/**
 * Settings form. Talks only through the asr client — no fetch, no token reads
 * (the value is write-only by design; presence is all JS ever sees).
 */
export function SettingsPanel() {
  const {
    config,
    saveConfig,
    saveToken,
    deleteToken,
    accessibilityTrusted,
    requestAccessibility,
    openAccessibilitySettings,
  } = useAsrClient();

  const [baseUrl, setBaseUrl] = useState("");
  const [hotkey, setHotkey] = useState("");
  const [cleanup, setCleanup] = useState(true);
  const [tokenDraft, setTokenDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);

  useEffect(() => {
    if (!config) return;
    setBaseUrl(config.baseUrl);
    setHotkey(config.hotkey);
    setCleanup(config.cleanup);
  }, [config]);

  if (!config) return null;

  const flash = (msg: string) => {
    setFeedback(msg);
    window.setTimeout(() => setFeedback(null), 2500);
  };

  const submitConfig = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await saveConfig({ baseUrl, hotkey, cleanup });
      flash("Saved");
    } catch (err) {
      // Rust rejects invalid accelerators; show its message.
      flash(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submitToken = async (e: FormEvent) => {
    e.preventDefault();
    if (!tokenDraft.trim()) return;
    setBusy(true);
    try {
      await saveToken(tokenDraft);
      setTokenDraft("");
      flash("Token stored in Keychain");
    } catch (err) {
      flash(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <form onSubmit={submitConfig} className="space-y-4">
        <div className="space-y-1.5">
          <Label htmlFor="base-url">Server URL</Label>
          <Input
            id="base-url"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="http://devbox:8090"
            spellCheck={false}
            autoComplete="off"
          />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="hotkey">Dictation hotkey</Label>
          <Input
            id="hotkey"
            value={hotkey}
            onChange={(e) => setHotkey(e.target.value)}
            placeholder="Cmd+Shift+Space"
            spellCheck={false}
            autoComplete="off"
          />
          <p className="text-xs text-muted-foreground">
            e.g. Cmd+Shift+Space, Ctrl+Option+V
          </p>
        </div>

        <div className="flex items-center justify-between">
          <div>
            <Label htmlFor="cleanup">Clean up transcript</Label>
            <p className="text-xs text-muted-foreground">
              Remove filler words before pasting
            </p>
          </div>
          <Switch
            id="cleanup"
            checked={cleanup}
            onCheckedChange={(v) => setCleanup(v === true)}
          />
        </div>

        <Button type="submit" variant="secondary" size="sm" disabled={busy}>
          Save settings
        </Button>
        {feedback && (
          <p className="text-xs text-muted-foreground">{feedback}</p>
        )}
      </form>

      <Separator />

      <form onSubmit={submitToken} className="space-y-1.5">
        <Label htmlFor="token">API token</Label>
        <div className="flex gap-2">
          <Input
            id="token"
            type="password"
            value={tokenDraft}
            onChange={(e) => setTokenDraft(e.target.value)}
            placeholder={config.hasToken ? "•••••• stored" : "asr_live_…"}
            autoComplete="off"
          />
          <Button type="submit" size="sm" variant="outline" disabled={busy}>
            <KeyRound /> Store
          </Button>
        </div>
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {config.hasToken
              ? "Stored in macOS Keychain — never sent to the UI."
              : "No token stored. Requests go out unauthenticated."}
          </span>
          {config.hasToken && (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-xs"
              disabled={busy}
              onClick={() => void deleteToken()}
            >
              Remove
            </Button>
          )}
        </div>
      </form>

      {accessibilityTrusted === false && (
        <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
          <div className="space-y-1.5">
            <p className="font-medium">Accessibility permission needed</p>
            <p className="text-muted-foreground">
              Without it, transcripts cannot be pasted into other apps.
            </p>
            <div className="flex gap-2">
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => void requestAccessibility()}
              >
                Request
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={openAccessibilitySettings}
              >
                Open System Settings
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
