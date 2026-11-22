// Observe how Obsidian + Academic Notes render notes (the behavioural reference for M3).
// Launches Obsidian with an ISOLATED profile on a COPY of the vault, opens each note in
// reading view and prints one JSON line per note: headings, callouts (type + rendered title),
// and every element the plugin added (class names starting with "an-"), with their text.
// usage: node observe.mjs <profileDir> <vaultCopy> <note.md>... [--html]
import { spawn } from "node:child_process";
import { writeFileSync, existsSync, mkdirSync } from "node:fs";

const args = process.argv.slice(2);
const html = args.includes("--html");
const [profile, vault, ...notes] = args.filter((a) => a !== "--html");
const port = 9334;
mkdirSync(profile, { recursive: true });
if (!existsSync(`${profile}/obsidian.json`)) {
  writeFileSync(`${profile}/obsidian.json`, JSON.stringify({ vaults: { m3vault000000001: { path: vault, ts: Date.now(), open: true } } }));
}
const child = spawn("electron43", ["/usr/lib/obsidian/app.asar", `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, `obsidian://open?path=${encodeURIComponent(vault)}`], { stdio: "ignore" });

async function ev(expr) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = targets.find((t) => t.type === "page" && !t.url.startsWith("devtools"));
  if (!page) return undefined;
  return await new Promise((resolve) => {
    const ws = new WebSocket(page.webSocketDebuggerUrl);
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression: expr, returnByValue: true, awaitPromise: true } }));
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id === 1) { ws.close(); resolve(d.result?.result?.value ?? d.result?.exceptionDetails?.text); } };
    ws.onerror = () => resolve(undefined);
  });
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let t = 0; ; t++) {
  try { if (await ev("!!(window.app && app.workspace && app.workspace.layoutReady)")) break; } catch {}
  await sleep(100);
  if (t > 600) { console.log("TIMEOUT"); child.kill(); process.exit(1); }
}
// isolated profile: leave restricted mode so the vault's plugins load (as in the user's profile)
await ev(`(async () => { if (!app.plugins.isEnabled()) { await app.plugins.setEnable(true); } for (const id of [...app.plugins.enabledPlugins]) await app.plugins.enablePlugin(id); return [...app.plugins.enabledPlugins]; })()`);
await sleep(3000); // Academic Notes indexes the vault
for (const note of notes) {
  const r = await ev(`(async () => { try {
    const f = app.vault.getAbstractFileByPath(${JSON.stringify(note)});
    const leaf = app.workspace.getLeaf(false);
    await leaf.openFile(f, { state: { mode: "preview" } });
    await new Promise(r => setTimeout(r, 1500));
    const src = await app.vault.cachedRead(f);
    const lineOf = (off) => src.slice(0, off).split(String.fromCharCode(10)).length;
    const pm = leaf.view.previewMode;
    const rr = pm.renderer;
    const sc = pm.containerEl.querySelector(".markdown-preview-view") || pm.containerEl;
    const t = (e) => (e.innerText || "").replace(/[ \\t\\n\\r]+/g, " ").trim();
    const sections = new Map(); // lineStart -> {headings, callouts, an}
    const collect = () => {
      for (const s of rr.sections) {
        if (!s.el || !s.el.isConnected || !s.rendered || sections.has(s.start)) continue;
        const e = s.el;
        if (!e.innerText.trim() && !e.querySelector("img,svg")) continue;
        const rec = { line: lineOf(s.start) };
        rec.headings = [...e.querySelectorAll("h1,h2,h3,h4,h5,h6")].map(h => h.tagName + " " + t(h));
        rec.callouts = [...e.querySelectorAll(".callout")].map(c => ({ type: c.dataset.callout, meta: c.dataset.calloutMetadata || "", title: t(c.querySelector(".callout-title") || c).slice(0, 120), cls: c.className }));
        const an = {};
        for (const x of e.querySelectorAll("[class*='an-']")) {
          const cls = [...x.classList].filter(c => c.startsWith("an-")).join(".");
          (an[cls] = an[cls] || []).push(t(x).slice(0, 80));
        }
        rec.an = an;
        ${html ? 'rec.html = e.innerHTML.slice(0, 20000);' : ''}
        sections.set(s.start, rec);
      }
    };
    for (let y = 0, n = 0; n < 400; n++) {
      sc.scrollTop = y;
      await new Promise(r => setTimeout(r, 250));
      collect();
      if (y > sc.scrollHeight) break;
      y += Math.max(200, sc.clientHeight * 0.7);
    }
    const out = { note: ${JSON.stringify(note)}, sections: [...sections.values()].sort((a, b) => a.line - b.line) };
    return JSON.stringify(out);
  } catch (e) { return "ERR " + e + " " + e.stack; } })()`);
  console.log(r);
}
child.kill();
process.exit(0);
