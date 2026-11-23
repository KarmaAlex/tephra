//! The toolbar's layout as you edit it in the app (M8): buttons, submenus and dividers, in
//! the JSON shape of the Editing Toolbar plugin's `menuCommands`, so a layout saved here reads
//! like one saved by the plugin. Fields this app doesn't use (the plugin's `icon`) are kept.
//! Commands this app doesn't have stay in the layout (shown as unavailable) unless you remove
//! them; the bar skips them.

use serde_json::{Map, Value};

pub const DIVIDER: &str = "editingToolbar-Divider-Line";
const SUBMENU_KEY: &str = "SubmenuCommands";

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    /// Other fields of the saved entry, written back unchanged.
    pub extra: Map<String, Value>,
    /// `Some` for a submenu (possibly empty).
    pub children: Option<Vec<Item>>,
}

impl Item {
    pub fn command(id: &str, name: &str) -> Self {
        Self { id: full_id(id), name: name.into(), extra: Map::new(), children: None }
    }
    pub fn divider(name: &str) -> Self {
        Self { id: DIVIDER.into(), name: name.into(), extra: Map::new(), children: None }
    }
    pub fn submenu(id: String, name: &str) -> Self {
        Self { id, name: name.into(), extra: Map::new(), children: Some(Vec::new()) }
    }
    pub fn is_divider(&self) -> bool {
        self.id.starts_with("editingToolbar-Divider")
    }
    pub fn is_submenu(&self) -> bool {
        self.children.is_some()
    }
    /// The command id without the plugin prefix (`toggle-bold`, `editor:insert-table`).
    pub fn command_id(&self) -> &str {
        self.id.trim_start_matches("editing-toolbar:")
    }
}

/// `toggle-bold` → `editing-toolbar:toggle-bold`, as the plugin saves command ids.
pub fn full_id(id: &str) -> String {
    if id.starts_with("editing-toolbar:") {
        id.to_string()
    } else {
        format!("editing-toolbar:{id}")
    }
}

/// Reads a saved `menuCommands` array (anything else gives an empty layout).
pub fn parse(v: &Value) -> Vec<Item> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let obj = it.as_object()?;
            let id = obj.get("id")?.as_str()?.to_string();
            let name = obj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let children = obj.get(SUBMENU_KEY).map(|s| parse(s).into_iter().filter(|c| !c.is_submenu()).collect());
            let extra = obj.iter().filter(|(k, _)| !matches!(k.as_str(), "id" | "name" | SUBMENU_KEY)).map(|(k, v)| (k.clone(), v.clone())).collect();
            Some(Item { id, name, extra, children })
        })
        .collect()
}

pub fn to_json(items: &[Item]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|it| {
                let mut o = Map::new();
                o.insert("id".into(), Value::from(it.id.clone()));
                o.insert("name".into(), Value::from(it.name.clone()));
                for (k, v) in &it.extra {
                    o.insert(k.clone(), v.clone());
                }
                if let Some(c) = &it.children {
                    o.insert(SUBMENU_KEY.into(), to_json(c));
                }
                Value::Object(o)
            })
            .collect(),
    )
}

/// Where an item is: its index at the top level, and its index inside that submenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Path {
    pub top: usize,
    pub child: Option<usize>,
}

impl Path {
    pub fn top(i: usize) -> Self {
        Self { top: i, child: None }
    }
    pub fn child(i: usize, j: usize) -> Self {
        Self { top: i, child: Some(j) }
    }
}

/// An insertion point: inside submenu `parent` (or the top level), before index `index`
/// (`index` = the list's length appends).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub parent: Option<usize>,
    pub index: usize,
}

impl Slot {
    /// Just before the item at `p`.
    pub fn before(p: Path) -> Self {
        match p.child {
            None => Self { parent: None, index: p.top },
            Some(j) => Self { parent: Some(p.top), index: j },
        }
    }
}

pub fn get(items: &[Item], p: Path) -> Option<&Item> {
    let top = items.get(p.top)?;
    match p.child {
        None => Some(top),
        Some(j) => top.children.as_ref()?.get(j),
    }
}

pub fn get_mut(items: &mut [Item], p: Path) -> Option<&mut Item> {
    let top = items.get_mut(p.top)?;
    match p.child {
        None => Some(top),
        Some(j) => top.children.as_mut()?.get_mut(j),
    }
}

fn list_mut(items: &mut Vec<Item>, parent: Option<usize>) -> Option<&mut Vec<Item>> {
    match parent {
        None => Some(items),
        Some(i) => items.get_mut(i)?.children.as_mut(),
    }
}

pub fn remove(items: &mut Vec<Item>, p: Path) -> Option<Item> {
    let list = list_mut(items, p.child.map(|_| p.top))?;
    let ix = p.child.unwrap_or(p.top);
    (ix < list.len()).then(|| list.remove(ix))
}

/// Inserts `item` at `at`; a submenu can't go inside a submenu (false, nothing changes).
pub fn insert(items: &mut Vec<Item>, at: Slot, item: Item) -> bool {
    if at.parent.is_some() && item.is_submenu() {
        return false;
    }
    let Some(list) = list_mut(items, at.parent) else { return false };
    if at.index > list.len() {
        return false;
    }
    list.insert(at.index, item);
    true
}

/// Moves the item at `from` to `to` (a slot computed before the move). False when the move
/// isn't possible (a submenu into a submenu, a bad path); nothing changes then.
pub fn move_item(items: &mut Vec<Item>, from: Path, to: Slot) -> bool {
    let Some(it) = get(items, from) else { return false };
    if to.parent.is_some() && it.is_submenu() {
        return false;
    }
    // into the submenu that is being moved: impossible
    if from.child.is_none() && to.parent == Some(from.top) {
        return false;
    }
    let same_list = match from.child {
        None => to.parent.is_none(),
        Some(_) => to.parent == Some(from.top),
    };
    let from_ix = from.child.unwrap_or(from.top);
    if same_list && (to.index == from_ix || to.index == from_ix + 1) {
        return true; // already there
    }
    let mut to = to;
    if same_list && to.index > from_ix {
        to.index -= 1;
    }
    // removing a top-level item before the target submenu shifts that submenu up
    if from.child.is_none() {
        if let Some(p) = to.parent {
            if p > from.top {
                to.parent = Some(p - 1);
            }
        }
    }
    let Some(it) = remove(items, from) else { return false };
    let backup = it.clone();
    if insert(items, to, it) {
        true
    } else {
        // put it back where it was
        let _ = insert(items, Slot::before(from), backup);
        false
    }
}

/// A submenu id no other entry has (`SubmenuCommands-<n>`).
pub fn new_submenu_id(items: &[Item]) -> String {
    (1..).map(|n| format!("SubmenuCommands-custom{n}")).find(|id| !items.iter().any(|i| &i.id == id)).unwrap()
}

/// Command ids used anywhere in the layout.
pub fn used_commands(items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    for it in items {
        match &it.children {
            Some(c) => out.extend(c.iter().filter(|c| !c.is_divider()).map(|c| c.command_id().to_string())),
            None if !it.is_divider() => out.push(it.command_id().to_string()),
            None => {}
        }
    }
    out
}

/// What the bar shows: commands this app has, submenus holding at least one of them (with
/// their section dividers), and top-level dividers between buttons.
pub fn shown(items: &[Item], known: &dyn Fn(&str) -> bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for it in items {
        if let Some(c) = &it.children {
            let kids: Vec<Item> = c.iter().filter(|c| c.is_divider() || known(c.command_id())).cloned().collect();
            if kids.iter().any(|k| !k.is_divider()) {
                out.push(Item { children: Some(kids), ..it.clone() });
            }
        } else if it.is_divider() || known(it.command_id()) {
            out.push(it.clone());
        }
    }
    // no dividers at the ends or twice in a row
    let mut clean: Vec<Item> = Vec::new();
    for it in out {
        if it.is_divider() && clean.last().is_none_or(|l| l.is_divider()) {
            continue;
        }
        clean.push(it);
    }
    while clean.last().is_some_and(|l| l.is_divider()) {
        clean.pop();
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match &i.children {
                Some(c) => format!("{}[{}]", i.id.trim_start_matches("SubmenuCommands-"), c.iter().map(|c| c.command_id()).collect::<Vec<_>>().join(",")),
                None => i.command_id().to_string(),
            })
            .collect()
    }

    fn sample() -> Vec<Item> {
        parse(&json!([
            {"id": "editing-toolbar:a", "name": "A", "icon": "bold-glyph"},
            {"id": "SubmenuCommands-s", "name": "submenu", "icon": "<svg/>", "SubmenuCommands": [
                {"id": "editing-toolbar:x", "name": "X"},
                {"id": DIVIDER, "name": "Section"},
                {"id": "editing-toolbar:y", "name": "Y"}
            ]},
            {"id": "editing-toolbar:b", "name": "B"},
            {"id": "editing-toolbar:c", "name": "C"}
        ]))
    }

    #[test]
    fn json_round_trip_keeps_unknown_fields() {
        let v = json!([
            {"id": "editing-toolbar:toggle-bold", "name": "Bold", "icon": "bold-glyph"},
            {"id": "SubmenuCommands-header", "name": "submenu", "icon": "header-n", "SubmenuCommands": [
                {"id": "editing-toolbar:header1-text", "name": "Header 1", "icon": "header-1"},
                {"id": DIVIDER, "name": "Line Operations", "icon": "vertical-split"}
            ]},
            {"id": "SubmenuCommands-empty", "name": "Empty", "SubmenuCommands": []}
        ]);
        let items = parse(&v);
        assert!(items[1].is_submenu() && items[2].is_submenu() && !items[0].is_submenu());
        assert!(items[1].children.as_ref().unwrap()[1].is_divider());
        assert_eq!(to_json(&items), v);
    }

    #[test]
    fn moves_within_and_between_lists() {
        // forward and backward at the top level
        let mut l = sample();
        assert!(move_item(&mut l, Path::top(0), Slot { parent: None, index: 3 }));
        assert_eq!(ids(&l), ["s[x,editingToolbar-Divider-Line,y]", "b", "a", "c"]);
        assert!(move_item(&mut l, Path::top(3), Slot { parent: None, index: 0 }));
        assert_eq!(ids(&l), ["c", "s[x,editingToolbar-Divider-Line,y]", "b", "a"]);
        // to the end
        let mut l = sample();
        assert!(move_item(&mut l, Path::top(0), Slot { parent: None, index: 4 }));
        assert_eq!(ids(&l), ["s[x,editingToolbar-Divider-Line,y]", "b", "c", "a"]);
        // dropping on itself or just after itself changes nothing
        let mut l = sample();
        assert!(move_item(&mut l, Path::top(2), Slot { parent: None, index: 3 }));
        assert!(move_item(&mut l, Path::top(2), Slot { parent: None, index: 2 }));
        assert_eq!(l, sample());
        // a button into a submenu (the button was before the submenu: indexes shift)
        let mut l = sample();
        assert!(move_item(&mut l, Path::top(0), Slot { parent: Some(1), index: 1 }));
        assert_eq!(ids(&l), ["s[x,a,editingToolbar-Divider-Line,y]", "b", "c"]);
        // out of a submenu, to the top level
        assert!(move_item(&mut l, Path::child(0, 0), Slot { parent: None, index: 3 }));
        assert_eq!(ids(&l), ["s[a,editingToolbar-Divider-Line,y]", "b", "c", "x"]);
        // inside a submenu, to its end
        assert!(move_item(&mut l, Path::child(0, 0), Slot { parent: Some(0), index: 3 }));
        assert_eq!(ids(&l), ["s[editingToolbar-Divider-Line,y,a]", "b", "c", "x"]);
        // a button after the submenu into it
        assert!(move_item(&mut l, Path::top(2), Slot { parent: Some(0), index: 0 }));
        assert_eq!(ids(&l), ["s[c,editingToolbar-Divider-Line,y,a]", "b", "x"]);
    }

    #[test]
    fn submenus_never_nest() {
        let mut l = sample();
        l.insert(0, Item::submenu("SubmenuCommands-t".into(), "T"));
        let before = l.clone();
        assert!(!move_item(&mut l, Path::top(0), Slot { parent: Some(2), index: 0 }), "a submenu into another");
        assert!(!move_item(&mut l, Path::top(2), Slot { parent: Some(2), index: 0 }), "a submenu into itself");
        assert!(!insert(&mut l, Slot { parent: Some(2), index: 0 }, Item::submenu("SubmenuCommands-u".into(), "U")));
        assert!(!move_item(&mut l, Path::top(9), Slot { parent: None, index: 0 }), "no such item");
        assert_eq!(l, before);
        // a submenu can still move along the top level
        assert!(move_item(&mut l, Path::top(2), Slot { parent: None, index: 0 }));
        assert_eq!(ids(&l)[0], "s[x,editingToolbar-Divider-Line,y]");
    }

    #[test]
    fn add_remove_and_ids() {
        let mut l = sample();
        assert!(insert(&mut l, Slot { parent: None, index: 4 }, Item::command("toggle-bold", "Bold")));
        assert_eq!(l[4].id, "editing-toolbar:toggle-bold");
        assert!(insert(&mut l, Slot { parent: Some(1), index: 3 }, Item::divider("")));
        assert_eq!(remove(&mut l, Path::child(1, 0)).map(|i| i.name), Some("X".into()));
        assert_eq!(remove(&mut l, Path::top(9)), None);
        assert_eq!(used_commands(&l), ["a", "y", "b", "c", "toggle-bold"]);
        let id = new_submenu_id(&l);
        l.push(Item::submenu(id.clone(), "Mine"));
        assert_ne!(new_submenu_id(&l), id);
    }

    #[test]
    fn the_bar_skips_unknown_commands_and_stray_dividers() {
        let l = parse(&json!([
            {"id": DIVIDER, "name": ""},
            {"id": "editing-toolbar:a", "name": "A"},
            {"id": "editing-toolbar:unknown", "name": "?"},
            {"id": DIVIDER, "name": ""},
            {"id": DIVIDER, "name": ""},
            {"id": "SubmenuCommands-only-unknown", "name": "", "SubmenuCommands": [{"id": "editing-toolbar:unknown", "name": ""}]},
            {"id": "SubmenuCommands-empty", "name": "", "SubmenuCommands": []},
            {"id": "SubmenuCommands-s", "name": "", "SubmenuCommands": [{"id": DIVIDER, "name": "Sec"}, {"id": "editing-toolbar:b", "name": ""}, {"id": "editing-toolbar:unknown", "name": ""}]},
            {"id": DIVIDER, "name": ""}
        ]));
        let shown = shown(&l, &|id| id == "a" || id == "b");
        assert_eq!(ids(&shown), ["a", "editingToolbar-Divider-Line", "s[editingToolbar-Divider-Line,b]"]);
    }
}
