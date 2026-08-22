export interface ExplorerPathProject {
  environment_type?: string;
  path: string;
  remote_path?: string;
}

export function normalizeExplorerRelativePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
}

export function parentExplorerRelativePath(relativePath: string): string {
  const normalized = normalizeExplorerRelativePath(relativePath);
  const index = normalized.lastIndexOf("/");
  return index === -1 ? "" : normalized.slice(0, index);
}

export function explorerPathSegments(relativePath: string): string[] {
  const normalized = normalizeExplorerRelativePath(relativePath);
  return normalized ? normalized.split("/").filter(Boolean) : [];
}

function posixRemoteRoot(remotePath?: string): string {
  const raw = (remotePath ?? "").trim();
  if (!raw || raw === "/") return "/";
  return raw.replace(/\/+$/g, "") || "/";
}

function stripRootPrefix(input: string, root: string, caseInsensitive: boolean): string | null {
  const normalizedInput = input.replace(/\\/g, "/").replace(/\/+$/g, "");
  const normalizedRoot = root.replace(/\\/g, "/").replace(/\/+$/g, "") || (root === "/" ? "/" : "");
  if (normalizedRoot === "/") {
    if (!normalizedInput || normalizedInput === "/") return "";
    return normalizedInput.startsWith("/") ? normalizedInput.slice(1) : null;
  }
  if (!normalizedRoot) return null;
  if (caseInsensitive) {
    if (normalizedInput.toLowerCase() === normalizedRoot.toLowerCase()) return "";
    const prefix = `${normalizedRoot.toLowerCase()}/`;
    if (!normalizedInput.toLowerCase().startsWith(prefix)) return null;
    return normalizedInput.slice(normalizedRoot.length + 1);
  }
  if (normalizedInput === normalizedRoot) return "";
  const prefix = `${normalizedRoot}/`;
  if (!normalizedInput.startsWith(prefix)) return null;
  return normalizedInput.slice(normalizedRoot.length + 1);
}

function validateRelativeSegments(relative: string): string | null {
  const parts = relative.split("/").filter(Boolean);
  if (parts.some((part) => part === "." || part === "..")) return null;
  return parts.join("/");
}

export function parseWslUncToLinuxPath(path: string): string | null {
  const normalized = path.trim().replace(/\\/g, "/");
  const match = /^\/\/(?:wsl\.localhost|wsl\$)\/[^/]+(\/.*)?$/i.exec(normalized);
  if (!match) return null;
  const linuxPath = match[1] ?? "/";
  return linuxPath || "/";
}

export function formatExplorerAddress(project: ExplorerPathProject, relativePath: string): string {
  const relative = normalizeExplorerRelativePath(relativePath);
  if (project.environment_type === "ssh") {
    const root = posixRemoteRoot(project.remote_path);
    if (!relative) return root;
    return root === "/" ? `/${relative}` : `${root}/${relative}`;
  }
  const root = project.path.replace(/[\\/]+$/g, "");
  if (!relative) return root;
  const separator = root.includes("\\") ? "\\" : "/";
  return `${root}${separator}${relative.replace(/\//g, separator)}`;
}

export function parseExplorerPathInput(
  input: string,
  project: ExplorerPathProject,
): string | null {
  const trimmed = input.trim();
  if (!trimmed || trimmed === "." || trimmed === "/" || trimmed === "\\") return "";

  const ssh = project.environment_type === "ssh";
  const asForward = trimmed.replace(/\\/g, "/");

  if (ssh) {
    const remoteRoot = posixRemoteRoot(project.remote_path);
    const underRemote = stripRootPrefix(asForward, remoteRoot, false);
    if (underRemote !== null) return validateRelativeSegments(underRemote);
    if (asForward.startsWith("/")) return null;
    return validateRelativeSegments(asForward.replace(/^\/+/g, ""));
  }

  const localRoot = project.path.replace(/[\\/]+$/g, "");
  const underLocal = stripRootPrefix(asForward, localRoot.replace(/\\/g, "/"), true);
  if (underLocal !== null) return validateRelativeSegments(underLocal);

  const wslLinux = parseWslUncToLinuxPath(project.path);
  if (wslLinux) {
    const underWsl = stripRootPrefix(asForward, wslLinux, false);
    if (underWsl !== null) return validateRelativeSegments(underWsl);
  }

  if (/^[A-Za-z]:\//.test(asForward) || asForward.startsWith("//") || asForward.startsWith("/")) {
    return null;
  }
  return validateRelativeSegments(asForward.replace(/^\/+/g, ""));
}

export function nextDuplicateName(name: string, attempt: number): string {
  const trimmed = name.trim() || "file";
  const index = trimmed.lastIndexOf(".");
  const hasExt = index > 0 && index < trimmed.length - 1 && !trimmed.slice(index + 1).includes(" ");
  const stem = hasExt ? trimmed.slice(0, index) : trimmed;
  const ext = hasExt ? trimmed.slice(index) : "";
  return attempt <= 1 ? `${stem} copy${ext}` : `${stem} copy ${attempt}${ext}`;
}

export function quoteExplorerShellPath(path: string, shell?: string | null): string {
  const key = (shell ?? "").toLowerCase();
  if (key === "powershell" || key === "pwsh") {
    return `'${path.replace(/'/g, "''")}'`;
  }
  if (key === "cmd") {
    return `"${path.replace(/"/g, '""')}"`;
  }
  return `'${path.replace(/'/g, `'\\''`)}'`;
}

export function resolveExplorerCdTarget(
  project: ExplorerPathProject,
  relativePath: string,
  shell?: string | null,
): string {
  if (project.environment_type === "ssh") {
    return formatExplorerAddress(project, relativePath);
  }
  const key = (shell ?? "").toLowerCase();
  if (key === "wsl" || key === "bash") {
    const linux = parseWslUncToLinuxPath(formatExplorerAddress(project, relativePath));
    if (linux) return linux;
  }
  return formatExplorerAddress(project, relativePath);
}

export function buildExplorerCdCommand(
  project: ExplorerPathProject,
  relativePath: string,
  shell?: string | null,
): string {
  const target = resolveExplorerCdTarget(project, relativePath, shell);
  const quoted = quoteExplorerShellPath(target, project.environment_type === "ssh" ? "bash" : shell);
  const key = (shell ?? "").toLowerCase();
  if (project.environment_type !== "ssh" && (key === "powershell" || key === "pwsh")) {
    return `Set-Location -LiteralPath ${quoted}`;
  }
  if (project.environment_type !== "ssh" && key === "cmd") {
    return `cd /d ${quoted}`;
  }
  return `cd ${quoted}`;
}
