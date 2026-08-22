import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("../src/lib/sshRemoteDirectories.ts", import.meta.url), "utf8");

function normalizeSshDirectoryPath(value) {
  const trimmed = value.trim();
  if (!trimmed) return "/";
  return trimmed === "/" ? "/" : trimmed.replace(/\/+$/, "") || "/";
}

function parentSshDirectoryPath(value) {
  const normalized = normalizeSshDirectoryPath(value);
  if (normalized === "/") return "/";
  const index = normalized.lastIndexOf("/");
  return index <= 0 ? "/" : normalized.slice(0, index);
}

function joinSshDirectoryPath(parent, name) {
  const cleanName = name.trim();
  if (!cleanName || cleanName.includes("/") || cleanName.includes("\\") || cleanName === "." || cleanName === "..") {
    throw new Error("ssh_remote_path_invalid");
  }
  const base = normalizeSshDirectoryPath(parent);
  return base === "/" ? `/${cleanName}` : `${base}/${cleanName}`;
}

test("SSH directory helpers are exported for the picker", () => {
  assert.match(source, /export function joinSshDirectoryPath/);
  assert.match(source, /export function parentSshDirectoryPath/);
  assert.match(source, /export async function createSshDirectory/);
  assert.match(source, /export async function deleteSshDirectory/);
  assert.match(source, /export async function resolveSshHomeDirectory/);
  assert.match(source, /ssh_home_directory/);
});

test("SSH directory path helpers join, parent, and reject escapes", () => {
  assert.equal(normalizeSshDirectoryPath("/home/dev/"), "/home/dev");
  assert.equal(joinSshDirectoryPath("/", "src"), "/src");
  assert.equal(joinSshDirectoryPath("/home/dev", "app"), "/home/dev/app");
  assert.equal(parentSshDirectoryPath("/home/dev/app"), "/home/dev");
  assert.equal(parentSshDirectoryPath("/"), "/");
  assert.throws(() => joinSshDirectoryPath("/home/dev", "../etc"), /ssh_remote_path_invalid/);
  assert.throws(() => joinSshDirectoryPath("/home/dev", "a/b"), /ssh_remote_path_invalid/);
});
