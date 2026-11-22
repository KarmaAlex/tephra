// Headless @codemirror/state + @lezer/markdown (GFM) for QuickJS. Used by the LaTeX snippets
// plugin when a function snippet reads `view.state` or `modifiedSyntaxTree(view.state)`.
// Build: `make cm6-bundle` -> crates/app/assets/cm6-headless.js
import { EditorState } from "@codemirror/state";
import { parser, GFM } from "@lezer/markdown";

const md = parser.configure(GFM);
const trees = new WeakMap();
globalThis.__cm6 = {
  state(doc) {
    return EditorState.create({ doc });
  },
  tree(state) {
    let t = trees.get(state);
    if (!t) {
      t = md.parse(state.doc.toString());
      trees.set(state, t);
    }
    return t;
  },
};
