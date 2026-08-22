import { useState } from "react";
import { ArrowUp, FolderPlus, Home, Trash2 } from "lucide-react";
import { useI18n } from "../../lib/i18n";
import type { SshDirectoryEntry } from "../../lib/sshRemoteDirectories";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

interface SshDirectoryPickerPanelProps {
  path: string;
  entries: SshDirectoryEntry[];
  loading: boolean;
  error: string;
  onPathChange: (path: string) => void;
  onLoad: (path: string, options?: { force?: boolean }) => void | Promise<void>;
  onHome: () => Promise<void>;
  onCreate: (name: string) => Promise<void>;
  onDelete: (path: string) => Promise<void>;
  pathLabel: string;
  emptyLabel: string;
}

function parentDirectory(path: string): string {
  return path.replace(/\/+$/, "").split("/").slice(0, -1).join("/") || "/";
}

export function SshDirectoryPickerPanel({
  path,
  entries,
  loading,
  error,
  onPathChange,
  onLoad,
  onHome,
  onCreate,
  onDelete,
  pathLabel,
  emptyLabel,
}: SshDirectoryPickerPanelProps) {
  const { t } = useI18n();
  const [creating, setCreating] = useState(false);
  const [folderName, setFolderName] = useState("");
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState("");

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setLocalError("");
    try {
      await action();
    } catch (nextError) {
      setLocalError(String(nextError));
    } finally {
      setBusy(false);
    }
  };

  const displayError = localError || error;

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={() => void onLoad(parentDirectory(path))}
          title={t("common.parentDirectory")}
          aria-label={t("common.parentDirectory")}
        >
          <ArrowUp className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={() => void run(onHome)}
          title={t("configModal.ssh.pickerHome")}
          aria-label={t("configModal.ssh.pickerHome")}
        >
          <Home className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={() => {
            setCreating((current) => !current);
            setFolderName("");
            setLocalError("");
          }}
          title={t("configModal.ssh.pickerNewFolder")}
          aria-label={t("configModal.ssh.pickerNewFolder")}
        >
          <FolderPlus className="h-4 w-4" />
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={busy || path === "/"}
          onClick={() => {
            if (path === "/") {
              setLocalError(t("configModal.ssh.pickerDeleteRoot"));
              return;
            }
            void run(async () => {
              await onDelete(path);
            });
          }}
          title={t("configModal.ssh.pickerDeleteFolder")}
          aria-label={t("configModal.ssh.pickerDeleteFolder")}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
        <Input
          value={path}
          aria-label={pathLabel}
          placeholder="/"
          onChange={(event) => onPathChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void onLoad(path);
          }}
          className="flex-1 font-mono text-sm"
        />
        <Button type="button" variant="outline" disabled={busy} onClick={() => void onLoad(path, { force: true })}>
          {t("common.refresh")}
        </Button>
      </div>
      {creating && (
        <div className="flex gap-2">
          <Input
            value={folderName}
            autoFocus
            placeholder={t("configModal.ssh.pickerFolderName")}
            aria-label={t("configModal.ssh.pickerFolderName")}
            onChange={(event) => setFolderName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && folderName.trim()) {
                void run(async () => {
                  await onCreate(folderName.trim());
                  setCreating(false);
                  setFolderName("");
                });
              }
              if (event.key === "Escape") {
                setCreating(false);
                setFolderName("");
              }
            }}
            className="flex-1 text-sm"
          />
          <Button
            type="button"
            disabled={busy || !folderName.trim()}
            onClick={() => void run(async () => {
              await onCreate(folderName.trim());
              setCreating(false);
              setFolderName("");
            })}
          >
            {t("common.confirm")}
          </Button>
        </div>
      )}
      <div className="max-h-80 min-h-52 overflow-y-auto rounded-xl border border-border bg-bg-secondary/60 p-1">
        {loading && <div className="p-4 text-sm text-text-muted">{t("common.loading")}</div>}
        {!loading && displayError && <div className="p-4 text-sm text-danger">{displayError}</div>}
        {!loading && !displayError && entries.length === 0 && <div className="p-4 text-sm text-text-muted">{emptyLabel}</div>}
        {!loading && !displayError && entries.map((entry) => (
          <button
            key={entry.path}
            type="button"
            onClick={() => onPathChange(entry.path)}
            onDoubleClick={() => void onLoad(entry.path)}
            className="ui-focus-ring flex w-full cursor-pointer items-center justify-between rounded-lg px-3 py-2 text-left text-sm text-text-primary transition-colors hover:bg-surface-container-highest"
          >
            <span className="truncate">{entry.name}</span>
            <span className="text-xs text-text-muted">›</span>
          </button>
        ))}
      </div>
    </div>
  );
}
