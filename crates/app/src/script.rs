//! Scripted input for testing the real app (OBSIDILIGHT_SCRIPT=<file>): feeds keystrokes and
//! mouse events through GPUI's normal dispatch (key bindings, text input handler, mouse
//! listeners), prints `STATE` lines, and can take screenshots with `spectacle`.
//!
//! Commands, one per line (`#` comments):
//!   wait <ms>                 key <keystroke> [n]     type <text>
//!   click <x> <y> [count]     drag <x1> <y1> <x2> <y2>
//!   scroll <dy>               state                   shot <path.png>     quit

use std::time::Duration;

use gpui_kit::*;

use crate::editor::Editor;

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

pub fn start(editor: Entity<Editor>, ws: WeakEntity<crate::workspace::Workspace>, window: &mut Window, cx: &mut App) {
    let Ok(script) = std::env::var("OBSIDILIGHT_SCRIPT").map(std::fs::read_to_string) else { return };
    let Ok(script) = script else { return };
    let handle = window.window_handle();
    let weak = editor.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(800)).await;
        for raw in script.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
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
                "clickat" => {
                    // clickat <offset> [ctrl]: click just right of where a document offset is drawn
                    let off = num(0) as usize;
                    let active = ws.update(cx, |ws, _| ws.tabs.get(ws.active).cloned()).ok().flatten().unwrap_or_else(|| editor.clone());
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
                    let active = ws.update(cx, |ws, _| ws.tabs.get(ws.active).cloned()).ok().flatten().unwrap_or_else(|| editor.clone());
                    let _ = active.update(cx, |e, cx| {
                        if let Some(off) = e.doc.text().find(&needle) {
                            let line = (e.doc.line_of(off) as i64 + delta).max(0) as usize;
                            let at = e.doc.line_start(line.min(e.doc.line_count() - 1));
                            e.set_selection(obsidilight_core::Selection::caret(at), false, cx);
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
                            ws.open_path(p, new_tab, w, cx);
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
                        let from = ws.tabs.get(ws.active).map(|e| e.read(cx).path.clone()).unwrap_or_default();
                        let r = ws.vault.borrow().resolve(&target, &from).map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                        println!("RESOLVE {target} -> {r:?}");
                    });
                }
                "ws-tabs" => {
                    let _ = ws.update(cx, |ws, cx| {
                        let root = ws.vault.borrow().root.clone();
                        let tabs: Vec<String> = ws.tabs.iter().map(|e| e.read(cx).path.strip_prefix(&root).map(|p| p.display().to_string()).unwrap_or_default()).collect();
                        println!("TABS active={} {:?}", ws.active, tabs);
                    });
                }
                "state" => {
                    let active = ws.update(cx, |ws, _| ws.tabs.get(ws.active).cloned()).ok().flatten();
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
                    let _ = std::process::Command::new("spectacle").args(["-a", "-b", "-n", "-o", rest.trim()]).status();
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
