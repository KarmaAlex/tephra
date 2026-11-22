# Generates src/vaultUtils.ts (~8k tokens) with one buried bug in parseSyncConflict.
import random
random.seed(7)
out = ['/* Assorted vault utilities. Generated for the model benchmark. */', '']
units = [("bytes","kib",1024),("kib","mib",1024),("mib","gib",1024),("ms","s",1000),("s","min",60),("min","h",60),("h","d",24),("mm","cm",10),("cm","m",100),("pt","px",4/3),("px","rem",1/16),("deg","rad",3.141592653589793/180)]
for a,b,f in units:
    out.append(f"""/** Convert {a} to {b}. */
export function {a}To{b.capitalize()}(value: number): number {{
  if (!Number.isFinite(value)) throw new TypeError("{a}To{b.capitalize()}: value must be finite");
  return value / {1/f if f<1 else f!r};
}}
""")
fields = ["title","alias","tag","cssclass","author","course","semester","professor","room","exam","topic","chapter","section","language","status","location","credits","grade","year","teacher","lab","project","deadline","priority","summary","source","license","version","category"]
for fld in fields:
    mx = random.choice([32,64,80,120,200])
    out.append(f"""/** Validate a `{fld}` frontmatter value. Returns an error message or null. */
export function validate{fld.capitalize()}(raw: unknown): string | null {{
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "{fld} must be a string";
  const v = raw.trim();
  if (v.length === 0) return "{fld} must not be empty";
  if (v.length > {mx}) return "{fld} is longer than {mx} characters";
  if (/[\\u0000-\\u001f]/.test(v)) return "{fld} contains control characters";
  return null;
}}
""")
exts = ["png","jpg","jpeg","gif","svg","webp","pdf","md","canvas","base","mp3","mp4","csv","json"]
out.append("export type FileKind = " + " | ".join(f'"{k}"' for k in ["image","document","note","media","data","other"]) + ";\n")
out.append("const KIND_BY_EXT: Record<string, FileKind> = {\n" + "".join(f'  {e}: "{ "image" if e in ("png","jpg","jpeg","gif","svg","webp") else "document" if e=="pdf" else "note" if e in ("md","canvas","base") else "media" if e in ("mp3","mp4") else "data"}",\n' for e in exts) + "};\n")
out.append('''/** Classify a vault path by extension. */
export function fileKind(path: string): FileKind {
  const dot = path.lastIndexOf(".");
  if (dot < 0 || dot < path.lastIndexOf("/")) return "other";
  return KIND_BY_EXT[path.slice(dot + 1).toLowerCase()] ?? "other";
}

/** Split a vault-relative path into folder and basename. */
export function splitPath(path: string): { folder: string; name: string } {
  const i = path.lastIndexOf("/");
  return i < 0 ? { folder: "", name: path } : { folder: path.slice(0, i), name: path.slice(i + 1) };
}

/** Join path segments with "/", dropping empty segments. */
export function joinPath(...parts: string[]): string {
  return parts.filter((p) => p.length > 0).join("/").replace(/\\/{2,}/g, "/");
}
''')
for i,(name,sep) in enumerate([("csvRow",","),("tsvRow","\\t"),("pipeRow","|"),("semicolonRow",";")]):
    out.append(f"""/** Split a {name} into trimmed cells, honouring double quotes. */
export function split{name[0].upper()+name[1:]}(line: string): string[] {{
  const cells: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {{
    const ch = line[i];
    if (ch === '"') {{
      if (quoted && line[i + 1] === '"') {{ cur += '"'; i++; }} else quoted = !quoted;
    }} else if (ch === "{sep}" && !quoted) {{
      cells.push(cur.trim());
      cur = "";
    }} else cur += ch;
  }}
  cells.push(cur.trim());
  return cells;
}}
""")
bug = '''export interface SyncConflict {
  /** Path of the original file, e.g. "Notes/a.md". */
  original: string;
  /** Conflict timestamp as "YYYYMMDD-HHMMSS". */
  stamp: string;
  /** Syncthing device short ID (7 characters, A-Z and 0-9). */
  device: string;
}

/**
 * Parse a Syncthing conflict file name such as
 * "Notes/a.sync-conflict-20250101-120000-ABCDEF1.md" -> { original: "Notes/a.md", stamp: "20250101-120000", device: "ABCDEF1" }.
 * Files without extension: "Notes/README.sync-conflict-20250101-120000-ABCDEF1" -> original "Notes/README".
 * Returns null for any other path.
 */
export function parseSyncConflict(path: string): SyncConflict | null {
  const m = /^(.*)\\.sync-conflict-(\\d{8})-(\\d{6})-([A-Z0-9]{6})(\\..*)?$/.exec(path);
  if (!m) return null;
  return { original: m[1] + m[5], stamp: m[2] + m[3], device: m[4] };
}
'''
fmts = ["DD-MM-YY","YYYY-MM-DD","DD/MM/YYYY","YYYYMMDD","DD.MM.YYYY","MM-DD-YYYY"]
for f in fmts:
    fn = "format" + "".join(c for c in f.title() if c.isalpha()) + "_" + str(fmts.index(f))
    out.append(f"""/** Format a date as {f}. */
export function {fn}(d: Date): string {{
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "{f}".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}}
""")
for i in range(24):
    name = random.choice(["clamp","lerp","roundTo","snap","wrap"]) + str(i)
    out.append(f"""/** Numeric helper #{i}. */
export function {name}(x: number, a: number, b: number): number {{
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * {10**(i%4)}) / {10**(i%4)};
}}
""")
out.insert(len(out)*11//20, bug)
open("src/vaultUtils.ts","w").write("\n".join(out))
