// Pure math-range scanner (no Node APIs) — shared by corpus tools and spikes.
export interface MathRange { from: number; to: number; display: boolean }

/** Same rules as bench task t4 (validated reference). */
export function findMathRanges(md: string): MathRange[] {
  const out: MathRange[] = [];
  const esc = (i: number) => { let n = 0; while (i - 1 - n >= 0 && md[i - 1 - n] === "\\") n++; return n % 2 === 1; };
  const fenceAt = (ls: number): [string, number] | null => {
    let k = ls, sp = 0;
    while (md[k] === " " && sp < 4) { k++; sp++; }
    // fences inside callouts/quotes: allow "> " prefixes
    while (md[k] === ">") { k++; while (md[k] === " ") k++; }
    if (sp > 3) return null;
    const c = md[k]; if (c !== "`" && c !== "~") return null;
    let n = 0; while (md[k + n] === c) n++;
    return n >= 3 ? [c, n] : null;
  };
  let i = 0;
  while (i < md.length) {
    if (i === 0 || md[i - 1] === "\n") {
      const f = fenceAt(i);
      if (f) {
        let p = md.indexOf("\n", i), end = md.length;
        while (p >= 0) {
          const g = fenceAt(p + 1);
          if (g && g[0] === f[0] && g[1] >= f[1]) { const e = md.indexOf("\n", p + 1); end = e < 0 ? md.length : e; break; }
          p = md.indexOf("\n", p + 1);
        }
        i = end; continue;
      }
    }
    const ch = md[i];
    if (ch === "`") {
      let n = 0; while (md[i + n] === "`") n++;
      let j = i + n, close = -1;
      while (j < md.length) { if (md[j] === "`") { let m = 0; while (md[j + m] === "`") m++; if (m === n) { close = j + m; break; } j += m; } else j++; }
      i = close >= 0 ? close : i + n; continue;
    }
    if (ch === "$" && !esc(i)) {
      if (md[i + 1] === "$") {
        let j = i + 2, close = -1;
        while (j < md.length - 1) { if (md[j] === "$" && md[j + 1] === "$" && !esc(j)) { close = j; break; } j++; }
        if (close >= 0) { out.push({ from: i, to: close + 2, display: true }); i = close + 2; } else i += 2;
        continue;
      }
      const nx = md[i + 1];
      if (nx !== undefined && !/\s/.test(nx)) {
        let j = i + 1, close = -1;
        while (j < md.length && md[j] !== "\n") {
          if (md[j] === "$" && !esc(j) && !/\s/.test(md[j - 1]) && !/[0-9]/.test(md[j + 1] ?? "")) { close = j; break; }
          j++;
        }
        if (close >= 0) { out.push({ from: i, to: close + 1, display: false }); i = close + 1; continue; }
      }
      i++; continue;
    }
    i++;
  }
  return out;
}

