import test from "node:test";
import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "url";

const here = dirname(fileURLToPath(import.meta.url));
const sourcePath = join(here, "../src/lib/sshRemoteDirectories.ts");

test("SSH directory path helpers join, parent, and reject escapes", async () => {
  const moduleUrl = `${pathToFileURL(sourcePath).href}?t=${Date.now()}`;
  const { joinSshDirectoryPath, parentSshDirectoryPath, normalizeSshDirectoryPath } = await import(moduleUrl);
  assert.equal(normalizeSshDirectoryPath("/home/dev/"), "/home/dev");
  assert.equal(joinSshDirectoryPath("/", "src"), "/src");
  assert.equal(joinSshDirectoryPath("/home/dev", "app"), "/home/dev/app");
  assert.equal(parentSshDirectoryPath("/home/dev/app"), "/home/dev");
  assert.equal(parentSshDirectoryPath("/"), "/");
  assert.throws(() => joinSshDirectoryPath("/home/dev", "../etc"), /ssh_remote_path_invalid/);
  assert.throws(() => joinSshDirectoryPath("/home/dev", "a/b"), /ssh_remote_path_invalid/);
});
