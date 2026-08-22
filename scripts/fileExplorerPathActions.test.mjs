import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const sidebar = read("../src/components/files/FileExplorerSidebar.tsx");
const formatter = read("../src/lib/aiPathFormatter.ts");
const drag = read("../src/lib/terminalFileDrag.ts");
const terminalInput = read("../src/hooks/useTerminalInput.ts");
const terminalTabs = read("../src/components/TerminalTabs.tsx");

test("terminal tab CLI icons inherit the terminal tab foreground color", () => {
  assert.equal((terminalTabs.match(/<CliToolIcon icon=\{cliToolIcon\} size=\{14\} className="text-current" \/>/g) ?? []).length, 3);
});

test("file menus expose relative and absolute path copy actions", () => {
  assert.match(sidebar, /import \{ PathCopyMenu \} from "\.\.\/PathCopyMenu"/);
  assert.equal((sidebar.match(/<PathCopyMenu /g) ?? []).length, 4);
});

test("file explorer exposes a path bar and extra file actions", () => {
  assert.match(sidebar, /<FileExplorerPathBar/);
  assert.match(sidebar, /files\.toolbar\.root/);
  assert.match(sidebar, /files\.path\.input/);
  assert.match(sidebar, /files\.menu\.cut/);
  assert.match(sidebar, /files\.menu\.duplicate/);
  assert.match(sidebar, /files\.menu\.copyName/);
  assert.match(sidebar, /files\.menu\.openInTerminal/);
  assert.match(sidebar, /files\.menu\.properties/);
  assert.match(sidebar, /files\.menu\.download/);
  assert.match(sidebar, /files\.menu\.upload/);
});

test("absolute file paths use the local root or SSH remote root", () => {
  assert.match(formatter, /project\.environment_type === "ssh" \? project\.remote_path : project\.path/);
  assert.ok(formatter.includes("normalizedPath.replace(/\\//g, separator)"));
});

test("file drags carry source context and absolute fallback data", () => {
  assert.match(drag, /export const TERMINAL_FILE_DRAG_MIME/);
  assert.match(drag, /absolutePath: formatAbsoluteProjectFilePath\(project, relativePath, kind\)/);
  assert.match(drag, /zone\.paste\(currentDrag\)/);
  assert.match(sidebar, /event\.dataTransfer\.setData\(TERMINAL_FILE_DRAG_MIME, JSON\.stringify\(payload\)\)/);
});

test("terminal drops choose relative text only for the same project location", () => {
  assert.match(terminalInput, /isSameProjectFileLocation\(payload\.source, targetProject\)/);
  assert.match(terminalInput, /payload\.absolutePath \|\| payload\.text/);
  assert.match(terminalInput, /projectWithWorktreePath\(project, worktree\)/);
  assert.match(terminalInput, /parseTerminalFileDragPayload\(event\.dataTransfer\?\.getData\(TERMINAL_FILE_DRAG_MIME\)\)/);
});

test("terminal file drags preserve the file panel and leave a command separator", () => {
  assert.match(drag, /suppressNextFilePanelProjectSync = true/);
  assert.match(terminalInput, /markTerminalFileDragPanelSyncSuppression\(\)/);
  assert.match(terminalTabs, /consumeTerminalFileDragPanelSyncSuppression\(\)/);
  assert.match(terminalInput, /appendTerminalFileDragSeparator\(resolveTerminalFileDragText\(payload\)\)/);
  assert.match(terminalInput, /payload \? appendTerminalFileDragSeparator\(text\) : text/);
});
