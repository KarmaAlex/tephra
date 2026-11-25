//! winshot: saves a screenshot of one window, chosen by its process id, as PNG.
//!
//!   winshot <pid> <out.png>
//!
//! On KDE Plasma (Wayland) through KWin's D-Bus API, in your normal session:
//! 1. a small KWin script finds the window of `<pid>` and calls back with its id;
//! 2. `org.kde.KWin.ScreenShot2.CaptureWindow` captures exactly that window.
//!
//! KWin only lets trusted programs capture, so this binary must be registered once with a
//! `.desktop` file that declares `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`
//! (`tools/winshot/install.sh` writes it; you run it). Nothing else on screen is captured.

use std::collections::HashMap;
use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::sync::mpsc;
use std::time::Duration;

use zbus::zvariant::{OwnedValue, Value};

const NAME: &str = "dev.tephra.WinShot";

struct Receiver(mpsc::Sender<String>);

#[zbus::interface(name = "dev.tephra.WinShot")]
impl Receiver {
    fn found(&self, id: String) {
        let _ = self.0.send(id);
    }
}

/// The KWin id (`{uuid}`) of the first window owned by `pid`.
fn window_id(conn: &zbus::blocking::Connection, rx: &mpsc::Receiver<String>, pid: u32) -> Result<String, String> {
    let script = format!(
        r#"const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
for (const w of list) {{
    if (w.pid === {pid} && !w.skipTaskbar) {{
        callDBus("{NAME}", "/", "{NAME}", "Found", w.internalId.toString());
        break;
    }}
}}"#
    );
    let path = std::env::temp_dir().join(format!("winshot-{}.js", std::process::id()));
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    let plugin = format!("winshot-{}", std::process::id());
    let scripting = zbus::blocking::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").map_err(|e| e.to_string())?;
    let id: i32 = scripting.call("loadScript", &(path.to_string_lossy().as_ref(), plugin.as_str())).map_err(|e| format!("loadScript: {e}"))?;
    let script_path = format!("/Scripting/Script{id}");
    let run = zbus::blocking::Proxy::new(conn, "org.kde.KWin", script_path.as_str(), "org.kde.kwin.Script")
        .and_then(|p| p.call::<_, _, ()>("run", &()))
        .map_err(|e| format!("run: {e}"));
    let found = run.and_then(|_| rx.recv_timeout(Duration::from_secs(3)).map_err(|_| format!("no window of process {pid}")));
    let _: Result<bool, _> = scripting.call("unloadScript", &(plugin.as_str(),));
    let _ = std::fs::remove_file(&path);
    found
}

fn capture(conn: &zbus::blocking::Connection, id: &str) -> Result<image::RgbaImage, String> {
    let (mut reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    // read concurrently: KWin writes the pixels into the pipe
    let pixels = std::thread::spawn(move || {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).map(|_| buf)
    });
    let proxy = zbus::blocking::Proxy::new(conn, "org.kde.KWin", "/org/kde/KWin/ScreenShot2", "org.kde.KWin.ScreenShot2").map_err(|e| e.to_string())?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("include-decoration", Value::from(true));
    opts.insert("include-shadow", Value::from(false));
    opts.insert("include-cursor", Value::from(false));
    opts.insert("native-resolution", Value::from(true));
    let fd: OwnedFd = writer.into();
    let meta: HashMap<String, OwnedValue> = proxy.call("CaptureWindow", &(id, opts, zbus::zvariant::Fd::from(&fd))).map_err(|e| {
        format!("CaptureWindow: {e}\n(is the .desktop file installed? run tools/winshot/install.sh)")
    })?;
    drop(fd);
    let data = pixels.join().map_err(|_| "reader thread".to_string())?.map_err(|e| e.to_string())?;
    let get = |k: &str| meta.get(k).and_then(|v| u32::try_from(v).ok()).ok_or_else(|| format!("no {k} in {meta:?}"));
    let (w, h, stride, format) = (get("width")?, get("height")?, get("stride")?, get("format")?);
    // QImage formats: 4/5/6 = (A)RGB32 (little endian: B G R A), 16/17/18 = RGBX/RGBA8888
    let bgra = match format {
        4..=6 => true,
        16..=18 => false,
        f => return Err(format!("unsupported QImage format {f}")),
    };
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        let row = &data[(y * stride) as usize..];
        for x in 0..w {
            let p = &row[(x * 4) as usize..(x * 4 + 4) as usize];
            let a = if format == 4 || format == 16 { 255 } else { p[3] };
            let px = if bgra { [p[2], p[1], p[0], a] } else { [p[0], p[1], p[2], a] };
            img.put_pixel(x, y, image::Rgba(px));
        }
    }
    Ok(img)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(pid), Some(out)) = (args.get(1).and_then(|p| p.parse::<u32>().ok()), args.get(2)) else {
        eprintln!("usage: winshot <pid> <out.png>");
        std::process::exit(2);
    };
    let (tx, rx) = mpsc::channel();
    let res = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at("/", Receiver(tx)))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
        .and_then(|conn| {
            let id = window_id(&conn, &rx, pid)?;
            capture(&conn, &id)
        })
        .and_then(|img| img.save(out).map_err(|e| e.to_string()));
    if let Err(e) = res {
        eprintln!("winshot: {e}");
        std::process::exit(1);
    }
}
