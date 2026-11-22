// What a Latex Suite snippets file can reach, inside QuickJS: `require("latex-suite")` and a
// lazily built `view.state` (CodeMirror state + Lezer Markdown tree) for function snippets.
// Everything else (DOM, Node APIs) is deliberately absent.
(function () {
  "use strict";
  var g = globalThis;
  var noop = function () {};
  g.console = { log: noop, warn: noop, error: noop, info: noop, debug: noop };
  g.window = g;
  var ls = { snippetVariables: {}, ALL_MACROS: [] };
  var api = {
    get snippetVariables() {
      return ls.snippetVariables;
    },
    get ALL_MACROS() {
      return ls.ALL_MACROS;
    },
    modifiedSyntaxTree: function (state) {
      return g.__cm6.tree(state);
    },
    addRawConcealMaps: noop,
    addMathlessMacros: noop,
    addUpdateHandler: noop,
  };
  g.require = function (name) {
    if (String(name).trim() === "latex-suite") return api;
    throw new Error('require("' + name + '") is not available in obsidilight');
  };
  g.__ls = ls;
  g.__lsFns = [];

  // Variables object -> {"${NAME}": value} (Latex Suite accepts both spellings).
  g.__lsSetVariables = function (raw) {
    var out = {};
    Object.keys(raw || {}).forEach(function (k) {
      var key = k.indexOf("${") === 0 ? k : "${" + k + "}";
      out[key] = String(raw[k]);
    });
    ls.snippetVariables = out;
    return JSON.stringify(out);
  };

  // Snippet objects -> JSON descriptors; function replacements stay here, by index.
  g.__lsDescribe = function (arr) {
    if (!Array.isArray(arr)) throw new Error("the snippets file must export an array");
    g.__lsFns = [];
    return JSON.stringify(
      arr.map(function (s, i) {
        var d = {
          i: i,
          options: s.options || "",
          priority: s.priority || 0,
          description: s.description || "",
          flags: s.flags || "",
          triggerKey: s.triggerKey || "",
          language: s.language === undefined ? null : String(s.language),
          excludedEnvironments: Array.isArray(s.excludedEnvironments) ? s.excludedEnvironments.map(String) : [],
        };
        if (s.trigger instanceof RegExp) {
          d.regex = s.trigger.source;
          d.flags = s.trigger.flags + d.flags;
        } else {
          d.trigger = s.trigger === undefined ? "" : String(s.trigger);
        }
        if (typeof s.replacement === "function") {
          d.fn = true;
          d.needsState = /state|modifiedSyntaxTree/.test(String(s.replacement));
          g.__lsFns[i] = s.replacement;
        } else if (typeof s.replacement === "string") {
          d.replacement = s.replacement;
        } else {
          d.unsupported = true;
        }
        return d;
      })
    );
  };

  // Runs function snippet `i`. `a`: {kind, match, index, input, from, to, doc}; positions in
  // UTF-16 units. Returns the replacement string, or null for "no expansion".
  g.__lsCall = function (a) {
    var state = null;
    var view = {
      get state() {
        if (!state) state = g.__cm6.state(a.doc());
        return state;
      },
    };
    var arg = a.match;
    if (a.kind === "regex") {
      arg.index = a.index;
      arg.input = a.input;
    }
    var captures = { match: a.kind === "regex" ? a.match.slice(1) : [a.match], groups: {} };
    var r = g.__lsFns[a.i](arg, { view: view, options: { captures: captures }, range: { from: a.from, to: a.to } });
    if (typeof r === "string") return r;
    if (r === false || r === null || r === undefined) return null;
    throw new Error("snippet " + a.i + ": only string or false replacements are supported");
  };

  // JS regex fallback for triggers the Rust engine cannot compile: [index, groups...] or null.
  g.__lsExec = function (i, source, flags, s) {
    var re = new RegExp("(?:" + source + ")$", flags);
    var m = re.exec(s);
    if (!m) return null;
    var out = [m.index];
    for (var k = 0; k < m.length; k++) out.push(m[k] === undefined ? null : m[k]);
    return out;
  };
})();
