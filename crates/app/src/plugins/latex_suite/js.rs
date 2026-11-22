//! QuickJS bridge: loads a Latex Suite snippets file (ESM `export default [...]`) and runs its
//! function replacements. Matching itself happens in Rust; JS is only entered when a function
//! snippet's trigger matched (or for a regex the Rust engine cannot compile).

use std::cell::Cell;
use std::rc::Rc;

use rquickjs::{CatchResultExt as _, Context, Function, Object, Runtime, Value};
use serde_json::Value as Json;

static SHIM: &str = include_str!("shim.js");
static CM6_BUNDLE: &str = include_str!("../../../assets/cm6-headless.js");
pub static ALL_MACROS: &str = include_str!("all_macros.txt");

pub struct Js {
    _rt: Runtime,
    ctx: Context,
    cm6_loaded: Cell<bool>,
}

/// What a function snippet receives as its first argument.
pub enum CallArg<'a> {
    /// Regex match: groups (index 0 = whole match), match index and input (UTF-16 index).
    Regex { groups: &'a [Option<String>], index: usize, input: &'a str },
    /// String snippet: the trigger.
    Str(&'a str),
    /// Visual snippet: the selection.
    Visual(&'a str),
}

/// `export default` -> a global assignment, so the file runs as a plain script.
fn as_script(src: &str, global: &str) -> String {
    match src.find("export default") {
        Some(i) => format!("{}globalThis.{global} = {}", &src[..i], &src[i + "export default".len()..]),
        None => src.to_string(),
    }
}

impl Js {
    pub fn new() -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(256 << 20);
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
        ctx.with(|ctx| -> Result<(), String> {
            ctx.eval::<(), _>(SHIM).catch(&ctx).map_err(|e| e.to_string())?;
            let macros: Vec<&str> = ALL_MACROS.lines().filter(|l| !l.is_empty()).collect();
            let ls: Object = ctx.globals().get("__ls").map_err(|e| e.to_string())?;
            ls.set("ALL_MACROS", macros).map_err(|e| e.to_string())?;
            Ok(())
        })?;
        Ok(Self { _rt: rt, ctx, cm6_loaded: Cell::new(false) })
    }

    /// Evaluates a variables file (`export default {...}`) and returns `{"${NAME}": value}`.
    pub fn load_variables(&self, src: &str) -> Result<Vec<(String, String)>, String> {
        self.ctx.with(|ctx| {
            ctx.eval::<(), _>(as_script(src, "__lsVarsRaw")).catch(&ctx).map_err(|e| format!("snippet variables: {e}"))?;
            let set: Function = ctx.globals().get("__lsSetVariables").map_err(|e| e.to_string())?;
            let raw: Value = ctx.globals().get("__lsVarsRaw").map_err(|e| e.to_string())?;
            let json: String = set.call((raw,)).catch(&ctx).map_err(|e| format!("snippet variables: {e}"))?;
            let v: serde_json::Map<String, Json> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(v.into_iter().map(|(k, v)| (k, v.as_str().unwrap_or_default().to_string())).collect())
        })
    }

    /// Evaluates the snippets file and returns one JSON descriptor per snippet.
    pub fn load_snippets(&self, src: &str) -> Result<Vec<Json>, String> {
        self.ctx.with(|ctx| {
            ctx.eval::<(), _>(as_script(src, "__lsRaw")).catch(&ctx).map_err(|e| format!("snippets file: {e}"))?;
            let describe: Function = ctx.globals().get("__lsDescribe").map_err(|e| e.to_string())?;
            let raw: Value = ctx.globals().get("__lsRaw").map_err(|e| e.to_string())?;
            let json: String = describe.call((raw,)).catch(&ctx).map_err(|e| format!("snippets file: {e}"))?;
            match serde_json::from_str::<Json>(&json).map_err(|e| e.to_string())? {
                Json::Array(a) => Ok(a),
                _ => Err("snippets file: expected an array".into()),
            }
        })
    }

    fn ensure_cm6(&self) -> Result<(), String> {
        if self.cm6_loaded.get() {
            return Ok(());
        }
        self.ctx.with(|ctx| ctx.eval::<(), _>(CM6_BUNDLE).catch(&ctx).map_err(|e| format!("cm6 bundle: {e}")))?;
        self.cm6_loaded.set(true);
        Ok(())
    }

    /// Runs function snippet `i`. `from`/`to` are UTF-16 offsets into `doc`. Ok(None) = the
    /// function declined (returned false).
    pub fn call(&self, i: usize, arg: CallArg, from: usize, to: usize, doc: Rc<String>, needs_state: bool) -> Result<Option<String>, String> {
        if needs_state {
            self.ensure_cm6()?;
        }
        self.ctx.with(|ctx| {
            let a = Object::new(ctx.clone()).map_err(|e| e.to_string())?;
            fn set_on<'js>(a: &Object<'js>, k: &str, v: Value<'js>) -> Result<(), String> {
                a.set(k, v).map_err(|e| e.to_string())
            }
            let set = |k: &str, v| set_on(&a, k, v);
            let s = |x: &str| rquickjs::String::from_str(ctx.clone(), x).map(|s| s.into_value()).map_err(|e| e.to_string());
            set("i", Value::new_int(ctx.clone(), i as i32))?;
            match arg {
                CallArg::Regex { groups, index, input } => {
                    set("kind", s("regex")?)?;
                    let arr = rquickjs::Array::new(ctx.clone()).map_err(|e| e.to_string())?;
                    for (k, g) in groups.iter().enumerate() {
                        let v = match g {
                            Some(g) => s(g)?,
                            None => Value::new_undefined(ctx.clone()),
                        };
                        arr.set(k, v).map_err(|e| e.to_string())?;
                    }
                    set("match", arr.into_value())?;
                    set("index", Value::new_int(ctx.clone(), index as i32))?;
                    set("input", s(input)?)?;
                }
                CallArg::Str(t) => {
                    set("kind", s("string")?)?;
                    set("match", s(t)?)?;
                }
                CallArg::Visual(t) => {
                    set("kind", s("visual")?)?;
                    set("match", s(t)?)?;
                }
            }
            set("from", Value::new_int(ctx.clone(), from as i32))?;
            set("to", Value::new_int(ctx.clone(), to as i32))?;
            let doc_fn = Function::new(ctx.clone(), move || doc.as_str().to_string()).map_err(|e| e.to_string())?;
            set("doc", doc_fn.into_value())?;
            let call: Function = ctx.globals().get("__lsCall").map_err(|e| e.to_string())?;
            let r: Option<String> = call.call((a,)).catch(&ctx).map_err(|e| e.to_string())?;
            Ok(r)
        })
    }

    /// JS-semantics regex search (fallback): Some((index, groups)) where index is in UTF-16
    /// units of `s`.
    pub fn exec(&self, source: &str, flags: &str, s: &str) -> Option<(usize, Vec<Option<String>>)> {
        self.ctx.with(|ctx| {
            let f: Function = ctx.globals().get("__lsExec").ok()?;
            let r: Option<Vec<Value>> = f.call((0, source, flags, s)).ok()?;
            let r = r?;
            let index = r.first()?.as_int()? as usize;
            let groups = r[1..].iter().map(|v| v.as_string().and_then(|s| s.to_string().ok())).collect();
            Some((index, groups))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_esm_snippets_and_calls_functions() {
        let js = Js::new().unwrap();
        let vars = js.load_variables("export default { \"${GREEK}\": \"alpha|beta\", \"SYM\": \"pm\" }").unwrap();
        assert!(vars.contains(&("${GREEK}".to_string(), "alpha|beta".to_string())));
        assert!(vars.contains(&("${SYM}".to_string(), "pm".to_string())));
        let src = r#"
            const helper = (x) => x + "!";
            export default [
              {trigger: "mk", replacement: "$$0$", options: "tA"},
              {trigger: /iden(\d)/, replacement: (m) => "I_" + m[1] + helper(""), options: "mA"},
              {trigger: /\\[A-Za-z]{2,}/, replacement: (m) => require("latex-suite").ALL_MACROS.includes(m[0]) ? m[0] : false, options: "mAU", priority: 3},
              {trigger: "lin", replacement: (m, {view, range}) => view.state.doc.lineAt(range.to).text + "|" + require("latex-suite").modifiedSyntaxTree(view.state).resolveInner(range.to, -1).name, options: "tA"},
            ]"#;
        let s = js.load_snippets(src).unwrap();
        assert_eq!(s.len(), 4);
        assert_eq!(s[0]["trigger"], "mk");
        assert_eq!(s[1]["regex"], "iden(\\d)");
        assert_eq!(s[1]["fn"], true);
        let g = [Some("iden3".to_string()), Some("3".to_string())];
        let r = js.call(1, CallArg::Regex { groups: &g, index: 0, input: "iden3" }, 0, 5, Rc::new(String::new()), false).unwrap();
        assert_eq!(r.as_deref(), Some("I_3!"));
        let g = [Some("\\alph".to_string())];
        assert_eq!(js.call(2, CallArg::Regex { groups: &g, index: 0, input: "" }, 0, 0, Rc::new(String::new()), false).unwrap(), None);
        let g = [Some("\\alpha".to_string())];
        assert_eq!(js.call(2, CallArg::Regex { groups: &g, index: 0, input: "" }, 0, 0, Rc::new(String::new()), false).unwrap().as_deref(), Some("\\alpha"));
        let doc = Rc::new("# T\n\nsome text li".to_string());
        let r = js.call(3, CallArg::Str("lin"), 15, 17, doc, true).unwrap();
        assert_eq!(r.as_deref(), Some("some text li|Paragraph"));
        assert_eq!(js.exec("a(b)?", "", "xxab"), Some((2, vec![Some("ab".into()), Some("b".into())])));
    }

    #[test]
    fn load_errors_are_reported() {
        let js = Js::new().unwrap();
        assert!(js.load_snippets("export default [ {trigger: ").unwrap_err().contains("snippets file"));
        assert!(js.load_snippets("export default {}").unwrap_err().contains("array"));
    }
}
