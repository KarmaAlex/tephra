// Launch Obsidian with an ISOLATED profile on the given vault, open a note, and print
// "READY" once the workspace is laid out and the note's view has rendered.
// The real ~/.config/obsidian is never touched.
// usage: node obsidian_run.mjs <profileDir> <vaultDir> <noteRelPath> [--enable-plugins]
import { spawn } from "node:child_process";
import { writeFileSync, existsSync } from "node:fs";

const [profile, vault, note] = process.argv.slice(2);
const enablePlugins = process.argv.includes("--enable-plugins");
const port = 9333;
if (!existsSync(`${profile}/obsidian.json`)) {
  writeFileSync(`${profile}/obsidian.json`, JSON.stringify({ vaults: { fixture0000000001: { path: vault, ts: Date.now(), open: true } } }));
}
const t0 = performance.now();
const child = spawn("electron43", ["/usr/lib/obsidian/app.asar", `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`,
  `obsidian://open?path=${encodeURIComponent(`${vault}/${note}`)}`], { stdio: ["ignore", "ignore", "ignore"] });
child.on("exit", (c) => { console.log("EXIT", c); process.exit(0); });

async function evalInPage(expr) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = targets.find((t) => t.type === "page" && !t.url.startsWith("devtools"));
  if (!page) return undefined;
  return await new Promise((resolve) => {
    const ws = new WebSocket(page.webSocketDebuggerUrl);
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression: expr, returnByValue: true, awaitPromise: true } }));
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id === 1) { ws.close(); resolve(d.result?.result?.value); } };
    ws.onerror = () => resolve(undefined);
  });
}

const READY_EXPR = `(() => { try { const f = app.workspace.getActiveFile(); const leaf = app.workspace.getMostRecentLeaf();
  return !!(app.workspace.layoutReady && f && f.path === ${JSON.stringify(note)} && leaf && leaf.view && leaf.view.contentEl.querySelector('.cm-content, .markdown-preview-sizer')) ? f.path : false; } catch { return false; } })()`;

for (;;) {
  try {
    const v = await evalInPage(READY_EXPR);
    if (v) break;
  } catch { /* not up yet */ }
  await new Promise((r) => setTimeout(r, 25));
  if (performance.now() - t0 > 60000) { console.log("TIMEOUT"); process.exit(1); }
}
console.log(`READY ${(performance.now() - t0).toFixed(1)}`);
const info = await evalInPage(`JSON.stringify({ version: (window.electron?.ipcRenderer?.sendSync?.('version')) ?? null, plugins: [...(app.plugins?.enabledPlugins ?? [])], restricted: !app.plugins?.isEnabled?.(), mode: app.workspace.getMostRecentLeaf().view.getMode?.() })`);
console.log(`INFO ${info}`);
if (enablePlugins && JSON.parse(info).restricted) {
  // Isolated profile only: turn off restricted mode, as the user has it in their own profile.
  await evalInPage(`(async () => { await app.plugins.setEnable(true); for (const id of [...app.plugins.enabledPlugins]) await app.plugins.enablePlugin(id); return true; })()`);
  console.log("PLUGINS_ENABLED");
}
setInterval(() => {}, 1 << 30);
