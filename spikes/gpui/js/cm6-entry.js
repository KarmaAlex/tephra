// Headless CM6 state + Lezer Markdown for QuickJS: the "emulation layer" is the real library.
import { EditorState } from "@codemirror/state";
import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { markdown } from "@codemirror/lang-markdown";
let state = null;
globalThis.cm6 = {
  load(doc) {
    state = EditorState.create({ doc, extensions: [markdown()] });
    const t0 = Date.now(); ensureSyntaxTree(state, state.doc.length, 5000); return Date.now() - t0;
  },
  // apply a single insertion like a keystroke, then reparse incrementally up to `pos`
  type(pos, text) {
    state = state.update({ changes: { from: pos, insert: text }, selection: { anchor: pos + text.length } }).state;
    ensureSyntaxTree(state, Math.min(state.doc.length, pos + 2000), 200);
    return state.doc.length;
  },
  // what the user's "dm" snippet does on trigger
  dmProbe(pos) {
    const tree = syntaxTree(state);
    const line = state.doc.lineAt(pos);
    let node = tree.resolveInner(pos, -1);
    const name = node.name;
    for (node = tree.resolveInner(line.from, 1); node.parent && node.parent.from === node.from && !node.parent.type.isTop; node = node.parent) {}
    const c = node.cursor(); let n = 0; do { n++; } while (c.next() && c.node.from < pos);
    return name + ":" + n;
  },
};
