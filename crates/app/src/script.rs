//! Scripted input for testing the real app (TEPHRA_SCRIPT=<file>): feeds keystrokes and
//! mouse events through GPUI's normal dispatch (key bindings, text input handler, mouse
//! listeners), prints `STATE` lines, and can take screenshots with `spectacle`.
//!
//! Commands, one per line (`#` comments):
//!   wait <ms>                 key <keystroke> [n]     type <text>
//!   click <x> <y> [count]     drag <x1> <y1> <x2> <y2>
//!   scroll <dy>               state                   shot <path.png>     quit
//!   resize <w> <h>             ws-root (the open vault, or none)
//!   bounds <key>              where a probed element is drawn (see [`probe`])
//!   popup                      the caret popup (inline math preview) of the active editor
//!   plugin-set <id> <key> <json>   change a plugin setting (the plugin reloads)
//!   ws-sort <order>            file list order (Obsidian's ids: alphabetical, byModifiedTime…)
//!   ws-order                   the file list's rows in display order
//!   clickk <key> [dx] [dy]     click a probed element's centre (plus an offset)
//!   rclickk <key> [dx] [dy]    right-click it (context menus)
//!   ws-panes                   the files open in each pane (`ws-panes`)
//!   ws-modes                   each pane's active tab and its view mode
//!   fonts                      theme font families + embedded-family resolution
//!   dragk <from-key> <to-key> [dy] [ms] [shot.png]   drag from one probed element's centre to
//!                             another's (dy: offset from the target's centre); then hold the
//!                             button `ms` before releasing, and/or take a screenshot
//! `shot` captures only this app's window, through `target/release/winshot` (see tools/winshot).

use std::time::Duration;

use gpui_kit::*;

use crate::editor::Editor;

/// Where elements marked with [`probe`] were last drawn; installed only while a script runs.
#[derive(Default)]
pub struct Probes(pub std::collections::HashMap<String, Bounds<Pixels>>);
impl Global for Probes {}

/// Records the bounds of its parent (which must be `relative`) under `key`, for scripts that
/// click or drag things whose position depends on layout. Nothing outside scripted runs.
pub fn probe(key: impl FnOnce() -> String, cx: &App) -> Option<AnyElement> {
    if !cx.has_global::<Probes>() {
        return None;
    }
    let key = key();
    Some(
        canvas(move |b, _, cx| {
            cx.global_mut::<Probes>().0.insert(key, b);
        }, |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

fn keystroke(s: &str) -> Option<Keystroke> {
    Keystroke::parse(Box::leak(s.to_string().into_boxed_str())).ok()
}

fn state(e: &Editor) -> String {
    let s = e.doc.selection();
    let line = e.doc.line_of(s.head);
    let ls = e.doc.line_start(line);
    let text = e.doc.line_text(line);
    let col = (s.head - ls).min(text.len());
    let shown: String = format!("{}|{}", &text[..col], &text[col..]).chars().take(120).collect();
    let caret = e
        .caret_bounds(s.head)
        .map(|b| format!("({:.0},{:.0} h{:.1})", f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.height)))
        .unwrap_or("none".into());
    format!(
        "STATE sel={}..{} line={} dirty={} caret_px={} text=\"{}\"",
        s.anchor,
        s.head,
        line + 1,
        e.doc.is_dirty(),
        caret,
        shown.replace('"', "'")
    )
}

/// A tab as UI tests print it: the note's vault path, or `view:<kind>`.
fn tab_name(t: &crate::workspace::Tab, root: &std::path::Path, cx: &App) -> String {
    match t {
        crate::workspace::Tab::Note(e) => e.read(cx).path.strip_prefix(root).map(|p| p.display().to_string()).unwrap_or_default(),
        crate::workspace::Tab::View(v) => format!("view:{}", v.kind),
    }
}

/// The window a running script drives, and its workspace and editor (if it has them).
#[derive(Clone)]
struct Target {
    window: AnyWindowHandle,
    editor: WeakEntity<Editor>,
    ws: WeakEntity<crate::workspace::Workspace>,
}
impl Global for Target {}

/// Runs the script on `window`. Called again while it runs (the welcome page opened a vault),
/// the script carries on in the new window.
pub fn start(editor: Option<Entity<Editor>>, ws: Option<WeakEntity<crate::workspace::Workspace>>, window: &mut Window, cx: &mut App) {
    let Ok(script) = std::env::var("TEPHRA_SCRIPT").map(std::fs::read_to_string) else { return };
    let Ok(script) = script else { return };
    let running = cx.has_global::<Target>();
    cx.set_global(Target {
        window: window.window_handle(),
        editor: editor.map(|e| e.downgrade()).unwrap_or_else(WeakEntity::new_invalid),
        ws: ws.unwrap_or_else(WeakEntity::new_invalid),
    });
    if running {
        return;
    }
    cx.set_global(Probes::default());
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(800)).await;
        for raw in script.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Target { window: handle, editor: weak, ws } = cx.update(|cx| cx.global::<Target>().clone());
            let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
            let num = |i: usize| rest.split_whitespace().nth(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            match cmd {
                "wait" => cx.background_executor().timer(Duration::from_millis(num(0) as u64)).await,
                "key" => {
                    let mut parts = rest.split_whitespace();
                    let k = parts.next().unwrap_or("");
                    let n: usize = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    for _ in 0..n {
                        if let Some(ks) = keystroke(k) {
                            let _ = handle.update(cx, |_, w, cx| w.dispatch_keystroke(ks, cx));
                        }
                        cx.background_executor().timer(Duration::from_millis(20)).await;
                    }
                }
                "type" => {
                    for ch in rest.chars() {
                        let k = if ch == ' ' { "space".to_string() } else { ch.to_string() };
                        if let Some(ks) = keystroke(&k) {
                            let _ = handle.update(cx, |_, w, cx| w.dispatch_keystroke(ks, cx));
                        }
                        cx.background_executor().timer(Duration::from_millis(20)).await;
                    }
                }
                "click" => {
                    // click <x> <y> [count] [ctrl]
                    let p = point(px(num(0)), px(num(1)));
                    let count = (num(2) as usize).max(1);
                    let modifiers = Modifiers { control: rest.split_whitespace().any(|w| w == "ctrl"), ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, modifiers, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p, click_count: count, modifiers, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p, click_count: count, modifiers, ..Default::default() }), cx);
                    });
                }
                "move" => {
                    // move <x> <y> [ctrl]: mouse moves there (hover)
                    let p = point(px(num(0)), px(num(1)));
                    let modifiers = Modifiers { control: rest.split_whitespace().any(|w| w == "ctrl"), ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, modifiers, ..Default::default() }), cx);
                    });
                }
                "moveat" => {
                    // moveat <offset> [ctrl]: hover just right of where a document offset is drawn
                    let off = num(0) as usize;
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let Some(b) = active.update(cx, |e, _| e.caret_bounds(off)) else {
                        println!("MOVEAT {off}: not on screen");
                        continue;
                    };
                    let p = point(b.origin.x + px(3.), b.center().y);
                    let modifiers = Modifiers { control: rest.split_whitespace().any(|w| w == "ctrl"), ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, modifiers, ..Default::default() }), cx);
                    });
                }
                "rclick" => {
                    // rclick <x> <y>: right click (context menus)
                    let p = point(px(num(0)), px(num(1)));
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Right, position: p, click_count: 1, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Right, position: p, click_count: 1, ..Default::default() }), cx);
                    });
                }
                "clickat" => {
                    // clickat <offset> [ctrl]: click just right of where a document offset is drawn
                    let off = num(0) as usize;
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let at = active.update(cx, |e, _| e.caret_bounds(off));
                    let Some(b) = at else {
                        println!("CLICKAT {off}: not on screen");
                        continue;
                    };
                    let p = point(b.origin.x + px(3.), b.center().y);
                    let modifiers = Modifiers { control: rest.split_whitespace().any(|w| w == "ctrl"), ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, modifiers, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p, click_count: 1, modifiers, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p, click_count: 1, modifiers, ..Default::default() }), cx);
                    });
                }
                "drag" => {
                    let (a, b) = (point(px(num(0)), px(num(1))), point(px(num(2)), px(num(3))));
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: a, click_count: 1, ..Default::default() }), cx);
                    });
                    for i in 1..=8 {
                        let t = i as f32 / 8.0;
                        let p = point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                        cx.background_executor().timer(Duration::from_millis(20)).await;
                        let _ = handle.update(cx, |_, w, cx| {
                            w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: Some(MouseButton::Left), ..Default::default() }), cx);
                        });
                    }
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: b, click_count: 1, ..Default::default() }), cx);
                    });
                }
                "bounds" => {
                    let b = cx.update(|cx| cx.global::<Probes>().0.get(rest.trim()).copied());
                    match b {
                        Some(b) => println!("BOUNDS {} {:.0} {:.0} {:.0} {:.0}", rest.trim(), f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)),
                        None => println!("BOUNDS {} none", rest.trim()),
                    }
                }
                "clickk" => {
                    let mut parts = rest.split_whitespace();
                    let k = parts.next().unwrap_or("");
                    let dx = parts.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                    let dy = parts.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                    let Some(b) = cx.update(|cx| cx.global::<Probes>().0.get(k).copied()) else {
                        println!("CLICKK {k}: not on screen");
                        continue;
                    };
                    let p = point(b.center().x + px(dx), b.center().y + px(dy));
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p, click_count: 1, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p, click_count: 1, ..Default::default() }), cx);
                    });
                }
                "rclickk" => {
                    let mut parts = rest.split_whitespace();
                    let k = parts.next().unwrap_or("");
                    let dx = parts.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                    let dy = parts.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                    let Some(b) = cx.update(|cx| cx.global::<Probes>().0.get(k).copied()) else {
                        println!("RCLICKK {k}: not on screen");
                        continue;
                    };
                    let p = point(b.center().x + px(dx), b.center().y + px(dy));
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Right, position: p, click_count: 1, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Right, position: p, click_count: 1, ..Default::default() }), cx);
                    });
                }
                "dragk" => {
                    let mut parts = rest.split_whitespace();
                    let (from, to) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    let dy = parts.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                    let (fb, tb) = cx.update(|cx| {
                        let p = &cx.global::<Probes>().0;
                        (p.get(from).copied(), p.get(to).copied())
                    });
                    let (Some(fb), Some(tb)) = (fb, tb) else {
                        println!("DRAGK {from} -> {to}: not on screen");
                        continue;
                    };
                    let (a, b) = (fb.center(), point(tb.center().x, tb.center().y + px(dy)));
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: a, ..Default::default() }), cx);
                        w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: a, click_count: 1, ..Default::default() }), cx);
                    });
                    for i in 1..=10 {
                        let t = i as f32 / 10.0;
                        let p = point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                        cx.background_executor().timer(Duration::from_millis(25)).await;
                        let _ = handle.update(cx, |_, w, cx| {
                            w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, pressed_button: Some(MouseButton::Left), ..Default::default() }), cx);
                        });
                    }
                    cx.background_executor().timer(Duration::from_millis(60)).await;
                    let mut last = parts.next();
                    if let Some(ms) = last.and_then(|v| v.parse::<u64>().ok()) {
                        // hold still (things that happen while resting on a spot), maybe then a shot
                        cx.background_executor().timer(Duration::from_millis(ms)).await;
                        last = parts.next();
                    }
                    if let Some(path) = last {
                        cx.background_executor().timer(Duration::from_millis(250)).await;
                        let tool = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("winshot")));
                        let ok = tool.as_ref().and_then(|t| std::process::Command::new(t).arg(std::process::id().to_string()).arg(path).status().ok()).is_some_and(|s| s.success());
                        println!("SHOT {path} {}", if ok { "ok" } else { "failed" });
                    }
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: b, click_count: 1, ..Default::default() }), cx);
                    });
                }
                "scroll" => {
                    let _ = handle.update(cx, |_, w, cx| {
                        let center = point(w.viewport_size().width / 2., w.viewport_size().height / 2.);
                        w.dispatch_event(PlatformInput::Scroll(ScrollEvent { position: center, delta: ScrollDelta::Pixels(point(px(0.), px(-num(0)))), ..Default::default() }), cx);
                    });
                }
                "pos" => {
                    // pos <offset>: screen position of a document offset, without moving the caret
                    let off = num(0) as usize;
                    let _ = weak.update(cx, |e, _| {
                        if let Some(b) = e.caret_bounds(off) {
                            println!("POS {off} {:.0} {:.0}", f32::from(b.origin.x), f32::from(b.center().y));
                        }
                    });
                }
                "find" => {
                    // find <text> [+lines]: caret at the start of the first occurrence, moved +/- lines
                    let (needle, delta) = match rest.rsplit_once(" @") {
                        Some((n, d)) => (n.to_string(), d.trim().parse::<i64>().unwrap_or(0)),
                        None => (rest.to_string(), 0),
                    };
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let _ = active.update(cx, |e, cx| {
                        if let Some(off) = e.doc.text().find(&needle) {
                            let line = (e.doc.line_of(off) as i64 + delta).max(0) as usize;
                            let at = e.doc.line_start(line.min(e.doc.line_count() - 1));
                            e.set_selection(tephra_core::Selection::caret(at), false, cx);
                        } else {
                            println!("FIND not found: {needle}");
                        }
                    });
                }
                "ws-open" => {
                    // ws-open <vault path> [new]
                    let new_tab = rest.ends_with(" new");
                    let rel = rest.trim_end_matches(" new").to_string();
                    let _ = handle.update(cx, |_, w, cx| {
                        let _ = ws.update(cx, |ws, cx| {
                            let p = ws.vault.borrow().root.join(&rel);
                            ws.open_file(p, new_tab, w, cx);
                        });
                    });
                }
                "ws-rename" => {
                    // ws-rename <old>|<new>
                    let (old, new) = rest.split_once('|').unwrap_or((rest, rest));
                    let r = ws.update(cx, |ws, cx| ws.rename_path(old.trim(), new.trim(), cx));
                    println!("RENAME {old} -> {new}: {:?}", r.ok());
                }
                "ws-resolve" => {
                    // ws-resolve <link target>: resolution from the active note, via the live index
                    let target = rest.trim().to_string();
                    let _ = ws.update(cx, |ws, cx| {
                        let from = ws.active_editor().map(|e| e.read(cx).path.clone()).unwrap_or_default();
                        let r = ws.vault.borrow().resolve(&target, &from).map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                        println!("RESOLVE {target} -> {r:?}");
                    });
                }
                "suggest" => {
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let _ = active.update(cx, |e, _| match &e.suggest {
                        Some(s) => println!("SUGGEST sel={} {:?}", s.selected, s.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()),
                        None => println!("SUGGEST none"),
                    });
                }
                "numbers" => {
                    // numbers: plugin callout labels of the active note, and the hover preview
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let _ = active.update(cx, |e, cx| {
                        let mut n: Vec<(usize, String)> = e.debug_numbers(cx).into_iter().collect();
                        n.sort();
                        println!("NUMBERS {:?}", n.into_iter().map(|(_, l)| l).collect::<Vec<_>>());
                        println!("HOVER {:?}", e.debug_hover());
                    });
                }
                "popup" => {
                    // popup: the caret popup (inline math preview) as of the last frame
                    let Some(active) = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten().or_else(|| weak.upgrade()) else {
                        println!("NO-EDITOR {cmd}");
                        continue;
                    };
                    let _ = active.update(cx, |e, _| println!("POPUP {:?}", e.debug_caret_popup()));
                }
                "plugin-set" => {
                    // plugin-set <id> <key> <json value>: change a plugin setting (reloads it)
                    let mut it = rest.splitn(3, ' ');
                    let (id, key, val) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("null"));
                    let val: serde_json::Value = serde_json::from_str(val).unwrap_or(serde_json::Value::Null);
                    cx.update(|cx| crate::plugins::PluginHost::set_setting(cx, id, key, val));
                }
                "ws-sort" => {
                    // ws-sort <order id>: change the file list's order (as the sort menu does)
                    if let Some(o) = tephra_core::sort::SortOrder::from_id(rest.trim()) {
                        let _ = ws.update(cx, |ws, cx| ws.set_sort(o, cx));
                    } else {
                        println!("BAD-SORT {rest}");
                    }
                }
                "ws-order" => {
                    // ws-order: every file-list row in display order (folders end with `/`)
                    let _ = ws.update(cx, |ws, _| println!("ORDER {} {:?}", ws.sort.id(), ws.tree_order));
                }
                "ws-right" => {
                    let _ = ws.update(cx, |ws, cx| ws.show_right_sidebar(true, cx));
                }
                "ws-tree" => {
                    let _ = ws.update(cx, |ws, _| {
                        let mut f = ws.tree_files.clone();
                        f.sort();
                        println!("TREE {f:?}");
                    });
                }
                "ws-panes" => {
                    // ws-panes: every pane (layout order) with its tabs; * = focused
                    let _ = ws.update(cx, |ws, cx| {
                        let root = ws.vault.borrow().root.clone();
                        let focused = ws.pane;
                        let mut out = Vec::new();
                        for id in ws.pane_ids() {
                            let tabs: Vec<String> = ws.pane_tabs(id).iter().map(|t| tab_name(t, &root, cx)).collect();
                            out.push(format!("{}{:?}", if id == focused { "*" } else { "" }, tabs));
                        }
                        println!("PANES {}", out.join(" | "));
                    });
                }
                "ws-modes" => {
                    // ws-modes: each pane's active tab and how it is shown (live, source, reading)
                    let _ = ws.update(cx, |ws, cx| {
                        let mut out = Vec::new();
                        for id in ws.pane_ids() {
                            let tabs = ws.pane_tabs(id);
                            let active = ws.pane_active(id);
                            let shown = match tabs.get(active.min(tabs.len().saturating_sub(1))) {
                                Some(crate::workspace::Tab::Note(e)) => format!("{}:{}", tab_name(&crate::workspace::Tab::Note(e.clone()), &ws.vault.borrow().root.clone(), cx), e.read(cx).mode.id()),
                                Some(t) => tab_name(t, &ws.vault.borrow().root.clone(), cx),
                                None => "-".to_string(),
                            };
                            out.push(format!("{}{shown}", if id == ws.pane { "*" } else { "" }));
                        }
                        println!("MODES {}", out.join(" | "));
                    });
                }
                "graph-click" | "graph-move" => {
                    // graph-click <node id> [ctrl]: click where the graph view draws that node
                    // graph-move <node id> [ctrl]: hover it
                    let (id, ctrl) = match rest.strip_suffix(" ctrl") {
                        Some(r) => (r.to_string(), true),
                        None => (rest.to_string(), false),
                    };
                    let view = crate::plugins::graph::LAST.with(|l| l.borrow().clone()).and_then(|w| w.upgrade());
                    let Some(p) = view.and_then(|v| v.update(cx, |g, _| g.screen_pos(&id))) else {
                        println!("GRAPH-CLICK {id}: not shown");
                        continue;
                    };
                    let modifiers = Modifiers { control: ctrl, ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p, modifiers, ..Default::default() }), cx);
                        if cmd == "graph-click" {
                            w.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p, click_count: 1, modifiers, ..Default::default() }), cx);
                            w.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p, click_count: 1, modifiers, ..Default::default() }), cx);
                        }
                    });
                }
                "theme" => {
                    // theme: light/dark, a few resolved variables for the active note, snippet reports
                    let active = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten();
                    let _ = cx.update(|cx| {
                        use gpui_kit::component::ActiveTheme as _;
                        let classes = active.map(|e| e.update(cx, |e, _| (*e.css_classes()).clone())).unwrap_or_default();
                        let dark = cx.theme().is_dark();
                        let Some(t) = cx.try_global::<crate::theme::ThemeCss>() else { return };
                        let bg = gpui_kit::Rgba::from(cx.theme().background);
                        println!(
                            "THEME dark={dark} bg=#{:02x}{:02x}{:02x} classes={classes:?} table-border={:?} accent={:?}",
                            (bg.r * 255.).round() as u8,
                            (bg.g * 255.).round() as u8,
                            (bg.b * 255.).round() as u8,
                            t.length("--table-border-width", dark, &classes, 16.0),
                            t.var("--interactive-accent", dark, &classes)
                        );
                        println!("THEMES {:?}", t.themes);
                        for r in &t.reports {
                            println!("THEME-FILE {} theme={} on={} global={} class={} {:?} ignored={}", r.name, r.is_theme, r.enabled, r.global_vars, r.class_vars, r.classes, r.ignored);
                        }
                    });
                }
                "mods" => {
                    // mods ctrl | mods none: modifier keys pressed or released on their own
                    let modifiers = Modifiers { control: rest.trim() == "ctrl", ..Default::default() };
                    let _ = handle.update(cx, |_, w, cx| {
                        w.dispatch_event(PlatformInput::ModifiersChanged(ModifiersChangedEvent { modifiers, ..Default::default() }), cx);
                    });
                }
                "proj" => {
                    // proj: the text of the active editor's first rendered leaf (what shows)
                    let active = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten();
                    if let Some(e) = active {
                        let _ = e.update(cx, |e, cx| {
                            let plain = e.plain;
                            let p = e.projection(cx);
                            let shown = p.blocks.first().map(|b| matches!(b.node, crate::editor::projection::ProjNode::Block(gpui_kit::base::text::editor_host::BlockNode::CodeBlock(_)))).unwrap_or(false);
                            println!("PROJ plain={plain} code_leaf={shown} blocks={}", p.blocks.len());
                        });
                    }
                }
                "ws-texts" => {
                    // ws-texts: the active note's text in every pane (layout order), `d` = unsaved
                    let _ = ws.update(cx, |ws, cx| {
                        let mut out = Vec::new();
                        for id in ws.pane_ids() {
                            let tabs = ws.pane_tabs(id);
                            let active = if id == ws.pane { ws.active } else { 0 };
                            if let Some(e) = tabs.get(active.min(tabs.len().saturating_sub(1))).and_then(|t| t.editor()) {
                                let e = e.read(cx);
                                out.push(format!("{}{:?}", if e.doc.is_dirty() { "d" } else { "" }, e.doc.text()));
                            }
                        }
                        println!("TEXTS {}", out.join(" | "));
                    });
                }
                "ws-root" => {
                    // ws-root: the open vault's folder (none: the welcome page)
                    match ws.update(cx, |ws, _| ws.vault.borrow().root.clone()) {
                        Ok(root) => println!("ROOT {}", root.display()),
                        Err(_) => println!("ROOT none"),
                    }
                }
                "ws-tabs" => {
                    let _ = ws.update(cx, |ws, cx| {
                        let root = ws.vault.borrow().root.clone();
                        let tabs: Vec<String> = ws.tabs.iter().map(|t| tab_name(t, &root, cx)).collect();
                        println!("TABS active={} {:?}", ws.active, tabs);
                    });
                }
                "fonts" => {
                    // fonts: the theme's families, and whether the embedded families resolved
                    let (sans, mono, names) = cx.update(|cx| {
                        let t = cx.global::<gpui_kit::component::theme::Theme>();
                        (t.font_family.to_string(), t.mono_font_family.to_string(), cx.text_system().all_font_names())
                    });
                    println!("FONTS sans={sans} mono={mono}");
                    for f in [crate::fonts::SANS_FAMILY, crate::fonts::MONO_FAMILY, "DejaVu Sans"] {
                        println!("FONT {f} {}", names.iter().any(|n| n == f));
                    }
                }
                "ws-focus" => {
                    // ws-focus [fullscreen]: toggle focus mode, optionally also full screen
                    let fullscreen = rest.trim() == "fullscreen";
                    let _ = handle.update(cx, |_, w, cx| {
                        let _ = ws.update(cx, |ws, cx| ws.toggle_focus_mode(fullscreen, w, cx));
                    });
                }
                "state" => {
                    let active = ws.update(cx, |ws, _| ws.active_editor()).ok().flatten();
                    match active {
                        Some(e) => {
                            let _ = e.update(cx, |e, _| println!("{}", state(e)));
                        }
                        None => {
                            let _ = weak.update(cx, |e, _| println!("{}", state(e)));
                        }
                    }
                }
                "shot" => {
                    // Only this app's own window, through KWin (tools/winshot): never the desktop.
                    let tool = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("winshot")));
                    let ok = tool.as_ref().and_then(|t| std::process::Command::new(t).arg(std::process::id().to_string()).arg(rest.trim()).status().ok()).is_some_and(|s| s.success());
                    println!("SHOT {} {}", rest.trim(), if ok { "ok" } else { "failed (build winshot and run tools/winshot/install.sh)" });
                }
                "resize" => {
                    // resize <w> <h>: the window's content size, in logical pixels
                    let _ = handle.update(cx, |_, w, _| w.resize(size(px(num(0)), px(num(1)))));
                }
                "quit" => {
                    let _ = cx.update(|cx| cx.quit());
                    return;
                }
                other => eprintln!("script: unknown command {other:?}"),
            }
            cx.background_executor().timer(Duration::from_millis(60)).await;
        }
    })
    .detach();
}
