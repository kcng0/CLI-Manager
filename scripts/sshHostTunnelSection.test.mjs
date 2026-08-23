import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "../src/components/settings/pages/SshHostTunnelSection.tsx"), "utf8");

test("SSH tunnel section does not filter forwards inside a Zustand selector", () => {
  assert.match(source, /useSshTunnelStore\(\(state\) => state\.forwards\)/);
  assert.doesNotMatch(source, /useSshTunnelStore\(\(state\) => hostForwards\(/);
});
