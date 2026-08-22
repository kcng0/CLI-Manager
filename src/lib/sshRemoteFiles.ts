import { invoke } from "@tauri-apps/api/core";
import type { Project, ProjectFileContentMatch, ProjectFileEntry, ProjectFilePreviewKind } from "./types";
import { buildSshAgentProjectLaunch, type SshAgentProjectLaunch } from "./sshAgentHistory";
import { useBackgroundOperationStore } from "../stores/backgroundOperationStore";
import type { TranslationKey } from "./i18n";

interface RemoteFileEntry {
  name: string;
  relativePath: string;
  kind: "file" | "directory" | string;
  sizeBytes: number;
  modifiedMs?: number | null;
}

interface RemoteFileRead {
  relativePath: string;
  kind: "text" | "image" | string;
  content: string;
  sizeBytes: number;
  modifiedMs?: number | null;
  truncated: boolean;
}

export interface SshRemoteFileContext {
  consumerId: string;
  launch: SshAgentProjectLaunch;
  rootPath: string;
}

function toEntry(entry: RemoteFileEntry): ProjectFileEntry {
  return {
    name: entry.name,
    path: entry.relativePath,
    kind: entry.kind === "directory" ? "directory" : "file",
    sizeBytes: entry.sizeBytes,
    modifiedMs: entry.modifiedMs ?? null,
  };
}

async function runFileOperation<T>(
  context: SshRemoteFileContext,
  detailKey: TranslationKey,
  action: () => Promise<T>,
): Promise<T> {
  const id = `remote-files:${context.consumerId}`;
  const retry = () => { void runFileOperation(context, detailKey, action).catch(() => undefined); };
  useBackgroundOperationStore.getState().start({
    id,
    kind: "remoteFiles",
    titleKey: "backgroundOperations.remoteFiles.title",
    detailKey,
    contextLabel: context.rootPath,
    retry,
  });
  try {
    const result = await action();
    useBackgroundOperationStore.getState().succeed(id);
    return result;
  } catch (error) {
    useBackgroundOperationStore.getState().fail(id, error);
    throw error;
  }
}

export async function buildSshRemoteFileContext(project: Project): Promise<SshRemoteFileContext> {
  const launch = await buildSshAgentProjectLaunch(project);
  return {
    consumerId: `files:${launch.clientInstanceId}:${launch.hostId}:${project.id}`,
    launch: {
      ...launch,
      bridgeEpoch: crypto.randomUUID(),
    },
    rootPath: project.remote_path.trim(),
  };
}

export type SshRemoteAttachmentInput =
  | { kind: "data"; fileName: string; dataBase64: string }
  | { kind: "localPath"; path: string };

export async function releaseSshRemoteFileContext(context: SshRemoteFileContext): Promise<void> {
  await invoke("history_remote_close", {
    hostId: context.launch.hostId,
    consumerId: context.consumerId,
  });
}

export async function sshRemoteAttachFiles(
  project: Project,
  sessionId: string,
  inputs: SshRemoteAttachmentInput[],
): Promise<string[]> {
  if (inputs.length === 0) return [];
  const context = await buildSshRemoteFileContext(project);
  context.consumerId = [
    "attachments",
    context.launch.clientInstanceId,
    context.launch.hostId,
    sessionId,
    crypto.randomUUID(),
  ].join(":");
  try {
    const paths: string[] = [];
    for (const input of inputs) {
      const common = {
        consumerId: context.consumerId,
        sshLaunch: context.launch,
        sessionId,
      };
      const path = input.kind === "data"
        ? await invoke<string>("ssh_remote_file_attach_data", {
            ...common,
            fileName: input.fileName,
            dataBase64: input.dataBase64,
          })
        : await invoke<string>("ssh_remote_file_attach_path", {
            ...common,
            localPath: input.path,
          });
      paths.push(path);
    }
    return paths;
  } finally {
    await releaseSshRemoteFileContext(context).catch(() => undefined);
  }
}

export async function sshRemoteListDir(
  context: SshRemoteFileContext,
  relativePath = "",
): Promise<ProjectFileEntry[]> {
  const response = await runFileOperation(context, "backgroundOperations.remoteFiles.listing", () =>
    invoke<{ entries: RemoteFileEntry[] }>("ssh_remote_file_list", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
    }));
  return (response.entries ?? []).map(toEntry);
}

export async function sshRemoteReadFile(
  context: SshRemoteFileContext,
  relativePath: string,
): Promise<{ content: string; previewKind: ProjectFilePreviewKind; sizeBytes: number; modifiedMs: number | null }> {
  const result = await runFileOperation(context, "backgroundOperations.remoteFiles.reading", () =>
    invoke<RemoteFileRead>("ssh_remote_file_read", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
    }));
  return {
    content: result.content,
    previewKind: result.kind === "image" ? "image" : "text",
    sizeBytes: result.sizeBytes,
    modifiedMs: result.modifiedMs ?? null,
  };
}

export async function sshRemoteSearch(
  context: SshRemoteFileContext,
  query: string,
  content = false,
): Promise<ProjectFileEntry[]> {
  const response = await runFileOperation(context, "backgroundOperations.remoteFiles.searching", () =>
    invoke<{ entries: RemoteFileEntry[] }>("ssh_remote_file_search", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      query,
      content,
    }));
  return (response.entries ?? []).map(toEntry);
}

export async function sshRemoteCreateEntry(
  context: SshRemoteFileContext,
  parentPath: string,
  name: string,
  kind: "file" | "directory",
  overwrite: boolean,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke("ssh_remote_file_create", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      parentPath,
      name,
      kind,
      overwrite,
    }));
}

export async function sshRemoteRenameEntry(
  context: SshRemoteFileContext,
  relativePath: string,
  newName: string,
  overwrite: boolean,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke("ssh_remote_file_rename", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
      newName,
      overwrite,
    }));
}

export async function sshRemoteDeleteEntry(
  context: SshRemoteFileContext,
  relativePath: string,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke("ssh_remote_file_delete", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
    }));
}

export async function sshRemoteTransferEntry(
  context: SshRemoteFileContext,
  mode: "copy" | "move",
  sourcePath: string,
  targetParentPath: string,
  name: string,
  overwrite: boolean,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke(mode === "copy" ? "ssh_remote_file_copy" : "ssh_remote_file_move", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      sourcePath,
      targetParentPath,
      name,
      overwrite,
    }));
}

export async function sshRemoteWriteText(
  context: SshRemoteFileContext,
  relativePath: string,
  content: string,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke("ssh_remote_file_write", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
      content,
    }));
}

export async function sshRemoteStat(
  context: SshRemoteFileContext,
  relativePath: string,
): Promise<ProjectFileEntry> {
  const entry = await runFileOperation(context, "backgroundOperations.remoteFiles.listing", () =>
    invoke<RemoteFileEntry>("ssh_remote_file_stat", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
    }));
  return toEntry(entry);
}

export async function sshRemoteReadBytes(
  context: SshRemoteFileContext,
  relativePath: string,
): Promise<{ name: string; sizeBytes: number; dataBase64: string }> {
  return runFileOperation(context, "backgroundOperations.remoteFiles.reading", () =>
    invoke("ssh_remote_file_read_bytes", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      relativePath,
    }));
}

export async function sshRemoteWriteBytes(
  context: SshRemoteFileContext,
  parentPath: string,
  name: string,
  dataBase64: string,
  overwrite: boolean,
): Promise<void> {
  await runFileOperation(context, "backgroundOperations.remoteFiles.writing", () =>
    invoke("ssh_remote_file_write_bytes", {
      consumerId: context.consumerId,
      sshLaunch: context.launch,
      rootPath: context.rootPath,
      parentPath,
      name,
      dataBase64,
      overwrite,
    }));
}

export function remoteEntryToSearchMatch(entry: ProjectFileEntry): ProjectFileContentMatch {
  return {
    path: entry.path,
    name: entry.name,
    lineNumber: 1,
    lineText: entry.name,
    before: [],
    after: [],
  };
}
