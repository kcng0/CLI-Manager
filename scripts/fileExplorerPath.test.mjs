import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const sourcePath = join(here, "../src/lib/fileExplorerPath.ts");
const source = readFileSync(sourcePath, "utf8");

test("file explorer path helpers are shared by the sidebar and tests", () => {
  assert.match(source, /export function parseExplorerPathInput/);
  assert.match(source, /export function formatExplorerAddress/);
  assert.match(source, /export function buildExplorerCdCommand/);
  assert.match(source, /export function nextDuplicateName/);
  assert.match(source, /parseWslUncToLinuxPath/);
});

test("path input accepts relative and rooted jumps and rejects escapes", async () => {
  const moduleUrl = `${pathToFileURL(sourcePath).href}?t=${Date.now()}`;
  const {
    parseExplorerPathInput,
    formatExplorerAddress,
    parentExplorerRelativePath,
    nextDuplicateName,
    buildExplorerCdCommand,
    parseWslUncToLinuxPath,
  } = await import(moduleUrl);

  const local = { environment_type: "local", path: "C:\\work\\app", remote_path: "" };
  const ssh = { environment_type: "ssh", path: "", remote_path: "/srv/app" };
  const sshRoot = { environment_type: "ssh", path: "", remote_path: "/" };
  const wsl = { environment_type: "wsl", path: "\\\\wsl.localhost\\Ubuntu\\home\\dev\\app", remote_path: "" };

  assert.equal(formatExplorerAddress(local, "src/lib"), "C:\\work\\app\\src\\lib");
  assert.equal(formatExplorerAddress(ssh, "src/lib"), "/srv/app/src/lib");
  assert.equal(formatExplorerAddress(ssh, ""), "/srv/app");
  assert.equal(parseExplorerPathInput("src/lib", local), "src/lib");
  assert.equal(parseExplorerPathInput("C:\\work\\app\\src\\lib", local), "src/lib");
  assert.equal(parseExplorerPathInput("C:/work/app", local), "");
  assert.equal(parseExplorerPathInput("/srv/app/src", ssh), "src");
  assert.equal(formatExplorerAddress(sshRoot, "src/lib"), "/src/lib");
  assert.equal(formatExplorerAddress(sshRoot, ""), "/");
  assert.equal(parseExplorerPathInput("/src", sshRoot), "src");
  assert.equal(parseExplorerPathInput("/", sshRoot), "");
  assert.equal(parseExplorerPathInput("src", sshRoot), "src");
  assert.equal(parseExplorerPathInput("../secret", local), null);
  assert.equal(parseExplorerPathInput("C:\\other\\app\\src", local), null);
  assert.equal(parseExplorerPathInput("/etc/passwd", ssh), null);
  assert.equal(parentExplorerRelativePath("src/lib/i18n.ts"), "src/lib");
  assert.equal(nextDuplicateName("notes.txt", 1), "notes copy.txt");
  assert.equal(nextDuplicateName("notes.txt", 2), "notes copy 2.txt");
  assert.equal(parseWslUncToLinuxPath(wsl.path), "/home/dev/app");
  assert.equal(buildExplorerCdCommand(ssh, "src", "powershell"), "cd '/srv/app/src'");
  assert.equal(buildExplorerCdCommand(local, "src", "powershell"), "Set-Location -LiteralPath 'C:\\work\\app\\src'");
  assert.equal(buildExplorerCdCommand(wsl, "src", "wsl"), "cd '/home/dev/app/src'");
});
