export interface Callout { type: string; title: string; fold: "+" | "-" | null; startLine: number; endLine: number; depth: number; children: Callout[] }
function depthOf(line: string): [number, string] {
  let k = 0, sp = 0; while (line[k] === " " && sp < 3) { k++; sp++; }
  let d = 0, last = -1;
  while (k < line.length) { if (line[k] === ">") { d++; last = k; k++; while (line[k] === " ") k++; } else break; }
  if (d === 0) return [0, line];
  let rest = line.slice(last + 1); if (rest.startsWith(" ")) rest = rest.slice(1);
  return [d, rest];
}
export function parseCallouts(md: string): Callout[] {
  const lines = md.split("\n").map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  const top: Callout[] = []; const stack: Callout[] = [];
  let fence = false, prevDepth = 0;
  lines.forEach((line, i) => {
    const [d, content] = depthOf(line);
    while (stack.length && d < stack[stack.length - 1].depth) stack.pop();
    if (d === 0 && /^\s{0,3}(```|~~~)/.test(line)) fence = !fence;
    if (!fence && d >= 1 && (i === 0 || prevDepth < d)) {
      const m = /^\[!([^\]]+)\]([+-]?)(.*)$/.exec(content);
      if (m) {
        const c: Callout = { type: m[1].toLowerCase(), title: m[3].trim(), fold: (m[2] || null) as any, startLine: i, endLine: i, depth: d, children: [] };
        while (stack.length && stack[stack.length - 1].depth >= d) stack.pop();
        (stack.length ? stack[stack.length - 1].children : top).push(c);
        stack.push(c);
      }
    }
    for (const c of stack) c.endLine = i;
    prevDepth = d;
  });
  return top;
}
