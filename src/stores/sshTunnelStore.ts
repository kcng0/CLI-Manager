import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { getDb } from "../lib/db";
import { translateCurrent } from "../lib/i18n";
import { buildSshConnectionSpec } from "../lib/ssh";
import type { SshPortForward, SshPortForwardMode, SshTunnelStatus } from "../lib/types";
import { useSshHostStore } from "./sshHostStore";

export interface SshForwardDraft {
  name: string;
  mode: SshPortForwardMode;
  listen_address: string;
  listen_port: number;
  target_host: string;
  target_port: number;
  auto_start: boolean;
}

interface SshTunnelStore {
  forwards: SshPortForward[];
  statuses: Record<string, SshTunnelStatus>;
  loaded: boolean;
  fetchForwards: () => Promise<void>;
  refreshStatuses: (ids?: string[]) => Promise<void>;
  saveForward: (hostId: string, draft: SshForwardDraft, id?: string) => Promise<SshPortForward>;
  deleteForward: (id: string) => Promise<void>;
  startForward: (id: string) => Promise<void>;
  stopForward: (id: string) => Promise<void>;
  startAutoForwards: () => Promise<void>;
}

function nowIso(): string {
  return new Date().toISOString();
}

function toForwardSpec(forward: SshPortForward) {
  if (forward.mode === "dynamic") {
    return {
      mode: "dynamic" as const,
      listenHost: forward.listen_address,
      listenPort: forward.listen_port,
    };
  }
  return {
    mode: forward.mode,
    listenHost: forward.listen_address,
    listenPort: forward.listen_port,
    targetHost: forward.target_host,
    targetPort: forward.target_port,
  };
}

function isValidPort(value: number): boolean {
  return Number.isInteger(value) && value >= 1 && value <= 65535;
}

function validateDraft(draft: SshForwardDraft): void {
  if (!isValidPort(draft.listen_port)) throw new Error("ssh_forward_port_invalid");
  if (draft.mode !== "dynamic" && !isValidPort(draft.target_port)) {
    throw new Error("ssh_forward_port_invalid");
  }
  if (!draft.listen_address.trim()) throw new Error("ssh_forward_host_invalid");
  if (draft.mode !== "dynamic" && !draft.target_host.trim()) throw new Error("ssh_forward_host_invalid");
}

export const EMPTY_FORWARD_DRAFT: SshForwardDraft = {
  name: "",
  mode: "local",
  listen_address: "127.0.0.1",
  listen_port: 8080,
  target_host: "127.0.0.1",
  target_port: 80,
  auto_start: false,
};

export const useSshTunnelStore = create<SshTunnelStore>((set, get) => ({
  forwards: [],
  statuses: {},
  loaded: false,

  fetchForwards: async () => {
    const db = await getDb();
    const forwards = await db.select<SshPortForward[]>(
      "SELECT * FROM ssh_port_forwards ORDER BY sort_order, name",
    );
    set({ forwards, loaded: true });
    await get().refreshStatuses(forwards.map((item) => item.id));
  },

  refreshStatuses: async (ids) => {
    const forwardIds = ids ?? get().forwards.map((item) => item.id);
    if (forwardIds.length === 0) {
      set({ statuses: {} });
      return;
    }
    const list = await invoke<SshTunnelStatus[]>("ssh_tunnel_list", { forwardIds });
    set({
      statuses: Object.fromEntries(list.map((item) => [item.forwardId, item])),
    });
  },

  saveForward: async (hostId, draft, id) => {
    validateDraft(draft);
    const db = await getDb();
    const existing = id ? get().forwards.find((item) => item.id === id) : null;
    const forward: SshPortForward = {
      id: existing?.id ?? crypto.randomUUID(),
      host_id: hostId,
      name: draft.name.trim(),
      mode: draft.mode,
      listen_address: draft.listen_address.trim() || "127.0.0.1",
      listen_port: draft.listen_port,
      target_host: draft.mode === "dynamic" ? "" : draft.target_host.trim(),
      target_port: draft.mode === "dynamic" ? 0 : draft.target_port,
      auto_start: draft.auto_start ? 1 : 0,
      sort_order: existing?.sort_order ?? get().forwards.filter((item) => item.host_id === hostId).length,
      created_at: existing?.created_at ?? nowIso(),
      updated_at: nowIso(),
    };
    if (existing) {
      await db.execute(
        `UPDATE ssh_port_forwards SET
           name = $1, mode = $2, listen_address = $3, listen_port = $4,
           target_host = $5, target_port = $6, auto_start = $7, updated_at = $8
         WHERE id = $9`,
        [
          forward.name, forward.mode, forward.listen_address, forward.listen_port,
          forward.target_host, forward.target_port, forward.auto_start, forward.updated_at, forward.id,
        ],
      );
    } else {
      await db.execute(
        `INSERT INTO ssh_port_forwards (
           id, host_id, name, mode, listen_address, listen_port, target_host, target_port,
           auto_start, sort_order, created_at, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)`,
        [
          forward.id, forward.host_id, forward.name, forward.mode, forward.listen_address,
          forward.listen_port, forward.target_host, forward.target_port, forward.auto_start,
          forward.sort_order, forward.created_at, forward.updated_at,
        ],
      );
    }
    await get().fetchForwards();
    if (existing && get().statuses[forward.id]?.state === "running") {
      await get().stopForward(forward.id);
      await get().startForward(forward.id);
    }
    return forward;
  },

  deleteForward: async (id) => {
    await get().stopForward(id).catch(() => undefined);
    const db = await getDb();
    await db.execute("DELETE FROM ssh_port_forwards WHERE id = $1", [id]);
    await get().fetchForwards();
  },

  startForward: async (id) => {
    const forward = get().forwards.find((item) => item.id === id);
    if (!forward) throw new Error("ssh_forward_not_found");
    const hosts = useSshHostStore.getState().hosts;
    const host = hosts.find((item) => item.id === forward.host_id);
    if (!host) throw new Error("ssh_host_not_found");
    const status = await invoke<SshTunnelStatus>("ssh_tunnel_start", {
      hostId: forward.host_id,
      forwardId: forward.id,
      spec: buildSshConnectionSpec(host, hosts),
      forward: toForwardSpec(forward),
    });
    set((state) => ({ statuses: { ...state.statuses, [id]: status } }));
  },

  stopForward: async (id) => {
    const status = await invoke<SshTunnelStatus>("ssh_tunnel_stop", { forwardId: id });
    set((state) => ({ statuses: { ...state.statuses, [id]: status } }));
  },

  startAutoForwards: async () => {
    await get().fetchForwards();
    const hosts = useSshHostStore.getState().hosts;
    if (hosts.length === 0) {
      await useSshHostStore.getState().fetchHosts();
    }
    let autoStartFailed = false;
    for (const forward of get().forwards.filter((item) => item.auto_start === 1)) {
      try {
        await get().startForward(forward.id);
      } catch {
        autoStartFailed = true;
      }
    }
    if (autoStartFailed) {
      toast.error(translateCurrent("settings.sshHosts.error.tunnelAutoStartFailed"));
    }
    await get().refreshStatuses();
  },
}));

export function hostForwards(forwards: SshPortForward[], hostId: string): SshPortForward[] {
  return forwards.filter((item) => item.host_id === hostId);
}
