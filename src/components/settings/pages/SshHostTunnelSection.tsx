import { useEffect, useState } from "react";
import { Play, Plus, Square, Trash2 } from "lucide-react";
import { useI18n, type TranslationKey } from "../../../lib/i18n";
import type { SshPortForward, SshPortForwardMode } from "../../../lib/types";
import {
  EMPTY_FORWARD_DRAFT,
  hostForwards,
  useSshTunnelStore,
  type SshForwardDraft,
} from "../../../stores/sshTunnelStore";
import { Select } from "../../ui/select";
import { useAppConfirm } from "../../ui/useAppConfirm";

function isLanBind(host: string): boolean {
  const value = host.trim().toLowerCase();
  return value === "0.0.0.0" || value === "::" || value === "[::]";
}

const TUNNEL_ERROR_LABELS: Record<string, TranslationKey> = {
  ssh_forward_port_invalid: "settings.sshHosts.error.forwardPortInvalid",
  ssh_forward_host_invalid: "settings.sshHosts.error.forwardHostInvalid",
  ssh_forward_not_found: "settings.sshHosts.error.forwardNotFound",
  ssh_host_not_found: "settings.sshHosts.error.notFound",
  ssh_interactive_auth_required: "settings.sshHosts.error.interactiveTunnel",
  ssh_tunnel_start_failed: "settings.sshHosts.error.tunnelStartFailed",
  ssh_tunnel_exited: "settings.sshHosts.error.tunnelExited",
};

function describeTunnelError(error: unknown, t: (key: TranslationKey) => string): string {
  const message = error instanceof Error ? error.message : String(error);
  const code = message.split(/[:\n]/)[0]?.trim() ?? message;
  const key = TUNNEL_ERROR_LABELS[code];
  return key ? t(key) : message;
}

function draftFromForward(forward: SshPortForward): SshForwardDraft {
  return {
    name: forward.name,
    mode: forward.mode,
    listen_address: forward.listen_address,
    listen_port: forward.listen_port,
    target_host: forward.target_host,
    target_port: forward.target_port || 80,
    auto_start: forward.auto_start === 1,
  };
}

export function SshHostTunnelSection({ hostId }: { hostId: string | null }) {
  const { t } = useI18n();
  // Zustand 5 的 useSyncExternalStore 要求 selector 返回稳定引用。
  // 不能在 selector 里 filter 出新数组，否则打开主机编辑器会把 React 打进无限更新。
  const allForwards = useSshTunnelStore((state) => state.forwards);
  const forwards = hostForwards(allForwards, hostId ?? "");
  const statuses = useSshTunnelStore((state) => state.statuses);
  const fetchForwards = useSshTunnelStore((state) => state.fetchForwards);
  const saveForward = useSshTunnelStore((state) => state.saveForward);
  const deleteForward = useSshTunnelStore((state) => state.deleteForward);
  const startForward = useSshTunnelStore((state) => state.startForward);
  const stopForward = useSshTunnelStore((state) => state.stopForward);
  const [draft, setDraft] = useState<SshForwardDraft>(EMPTY_FORWARD_DRAFT);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const { confirm, confirmDialog } = useAppConfirm();

  useEffect(() => {
    if (hostId) void fetchForwards();
  }, [fetchForwards, hostId]);

  if (!hostId) {
    return <div className="rounded-xl border border-border bg-surface-low px-3 py-2 text-xs leading-relaxed text-text-muted">{t("settings.sshHosts.tunnels.saveFirst")}</div>;
  }

  const setDraftValue = <K extends keyof SshForwardDraft>(key: K, value: SshForwardDraft[K]) => {
    setDraft((current) => ({ ...current, [key]: value }));
    setError("");
  };

  const resetDraft = () => {
    setDraft(EMPTY_FORWARD_DRAFT);
    setEditingId(null);
    setError("");
  };

  const persist = async () => {
    setBusyId(editingId ?? "new");
    setError("");
    try {
      await saveForward(hostId, draft, editingId ?? undefined);
      resetDraft();
    } catch (nextError) {
      setError(describeTunnelError(nextError, t));
    } finally {
      setBusyId(null);
    }
  };

  const run = async (id: string, action: "start" | "stop" | "delete") => {
    if (action === "delete") {
      const forward = forwards.find((item) => item.id === id);
      const confirmed = await confirm({
        title: t("settings.sshHosts.tunnels.delete"),
        message: t("settings.sshHosts.tunnels.deleteConfirm", { name: forward?.name || id }),
        danger: true,
      });
      if (!confirmed) return;
    }
    setBusyId(id);
    setError("");
    try {
      if (action === "start") await startForward(id);
      else if (action === "stop") await stopForward(id);
      else await deleteForward(id);
      if (action === "delete" && editingId === id) resetDraft();
    } catch (nextError) {
      setError(describeTunnelError(nextError, t));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="space-y-3">
      {confirmDialog}
      {error && <div className="rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">{error}</div>}
      {forwards.length === 0 && <div className="rounded-xl border border-border bg-surface-low px-3 py-2 text-xs text-text-muted">{t("settings.sshHosts.tunnels.empty")}</div>}
      {forwards.map((forward) => {
        const status = statuses[forward.id];
        const state = status?.state ?? "stopped";
        const tone = state === "running" ? "text-emerald-500" : state === "error" ? "text-red-500" : "text-text-muted";
        const label = state === "running"
          ? t("settings.sshHosts.tunnels.running")
          : state === "error"
            ? t("settings.sshHosts.tunnels.error")
            : t("settings.sshHosts.tunnels.stopped");
        const summary = forward.mode === "dynamic"
          ? `${forward.listen_address}:${forward.listen_port}`
          : `${forward.listen_address}:${forward.listen_port} → ${forward.target_host}:${forward.target_port}`;
        return (
          <div key={forward.id} className="rounded-xl border border-border bg-surface-lowest px-4 py-3">
            <div className="flex items-start justify-between gap-3">
              <button type="button" className="min-w-0 flex-1 text-left" onClick={() => { setEditingId(forward.id); setDraft(draftFromForward(forward)); setError(""); }}>
                <div className="truncate text-sm font-bold text-text-primary">{forward.name || t(`settings.sshHosts.tunnels.mode.${forward.mode}` as const)}</div>
                <div className="mt-1 truncate font-mono text-[11px] text-text-muted">{summary}</div>
                <div className={`mt-1 text-[11px] font-bold ${tone}`}>{label}{status?.error ? ` · ${describeTunnelError(status.error, t)}` : ""}</div>
              </button>
              <div className="flex shrink-0 gap-1">
                {state === "running" ? (
                  <button type="button" className="ui-icon-button h-8 w-8" title={t("settings.sshHosts.tunnels.stop")} aria-label={t("settings.sshHosts.tunnels.stop")} disabled={busyId === forward.id} onClick={() => void run(forward.id, "stop")}>
                    <Square className="h-3.5 w-3.5" />
                  </button>
                ) : (
                  <button type="button" className="ui-icon-button h-8 w-8" title={t("settings.sshHosts.tunnels.start")} aria-label={t("settings.sshHosts.tunnels.start")} disabled={busyId === forward.id} onClick={() => void run(forward.id, "start")}>
                    <Play className="h-3.5 w-3.5" />
                  </button>
                )}
                <button type="button" className="ui-icon-button h-8 w-8 text-danger" title={t("settings.sshHosts.tunnels.delete")} aria-label={t("settings.sshHosts.tunnels.delete")} disabled={busyId === forward.id} onClick={() => void run(forward.id, "delete")}>
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </div>
        );
      })}
      <TunnelDraftForm
        draft={draft}
        saving={busyId === (editingId ?? "new")}
        onChange={setDraftValue}
        onSave={() => void persist()}
        onReset={resetDraft}
      />
    </div>
  );
}

function TunnelDraftForm({
  draft,
  saving,
  onChange,
  onSave,
  onReset,
}: {
  draft: SshForwardDraft;
  saving: boolean;
  onChange: <K extends keyof SshForwardDraft>(key: K, value: SshForwardDraft[K]) => void;
  onSave: () => void;
  onReset: () => void;
}) {
  const { t } = useI18n();
  const dynamic = draft.mode === "dynamic";
  return (
    <div className="space-y-3 rounded-xl border border-border bg-surface-lowest px-4 py-3">
      <div className="grid grid-cols-2 gap-3">
        <label className="space-y-1 text-xs">
          <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.name")}</span>
          <input className="h-9 w-full rounded-lg border border-border bg-surface-low px-3 text-sm" value={draft.name} placeholder={t("settings.sshHosts.tunnels.namePlaceholder")} onChange={(event) => onChange("name", event.target.value)} />
        </label>
        <label className="space-y-1 text-xs">
          <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.mode")}</span>
          <Select className="h-9 text-sm" value={draft.mode} onChange={(event) => onChange("mode", event.target.value as SshPortForwardMode)}>
            <option value="local">{t("settings.sshHosts.tunnels.mode.local")}</option>
            <option value="remote">{t("settings.sshHosts.tunnels.mode.remote")}</option>
            <option value="dynamic">{t("settings.sshHosts.tunnels.mode.dynamic")}</option>
          </Select>
        </label>
        <label className="space-y-1 text-xs">
          <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.listen")}</span>
          <input className="h-9 w-full rounded-lg border border-border bg-surface-low px-3 font-mono text-sm" value={draft.listen_address} onChange={(event) => onChange("listen_address", event.target.value)} />
        </label>
        <label className="space-y-1 text-xs">
          <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.listenPort")}</span>
          <input className="h-9 w-full rounded-lg border border-border bg-surface-low px-3 text-sm" type="number" min={1} max={65535} value={draft.listen_port} onChange={(event) => onChange("listen_port", Number(event.target.value))} />
        </label>
        {!dynamic && (
          <>
            <label className="space-y-1 text-xs">
              <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.target")}</span>
              <input className="h-9 w-full rounded-lg border border-border bg-surface-low px-3 font-mono text-sm" value={draft.target_host} onChange={(event) => onChange("target_host", event.target.value)} />
            </label>
            <label className="space-y-1 text-xs">
              <span className="font-bold text-text-primary">{t("settings.sshHosts.tunnels.targetPort")}</span>
              <input className="h-9 w-full rounded-lg border border-border bg-surface-low px-3 text-sm" type="number" min={1} max={65535} value={draft.target_port} onChange={(event) => onChange("target_port", Number(event.target.value))} />
            </label>
          </>
        )}
      </div>
      <label className="flex items-center gap-2 text-xs font-bold text-text-primary">
        <input type="checkbox" checked={draft.auto_start} onChange={(event) => onChange("auto_start", event.target.checked)} />
        {t("settings.sshHosts.tunnels.autoStart")}
      </label>
      {isLanBind(draft.listen_address) && (
        <div className="rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">{t("settings.sshHosts.tunnels.lanBindWarning")}</div>
      )}
      <div className="flex justify-end gap-2">
        <button type="button" className="ui-button-secondary h-8 rounded-lg px-3 text-xs font-bold" onClick={onReset}>{t("common.cancel")}</button>
        <button type="button" className="ui-button-primary h-8 rounded-lg px-3 text-xs font-bold" disabled={saving} onClick={onSave}>
          <Plus className="mr-1 inline h-3.5 w-3.5" />
          {saving ? t("common.saving") : t("settings.sshHosts.tunnels.add")}
        </button>
      </div>
    </div>
  );
}
