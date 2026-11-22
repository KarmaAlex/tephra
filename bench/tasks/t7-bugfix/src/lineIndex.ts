/** Maps between string offsets and (line, column) positions. Lines are separated by "\n". */
export class LineIndex {
  private readonly starts: number[];
  constructor(private readonly text: string) {
    this.starts = [0];
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "\n") this.starts.push(i);
    }
  }
  get lineCount(): number {
    return this.starts.length;
  }
  /** 0-based line containing offset `pos` (0 <= pos <= text.length). */
  lineAt(pos: number): number {
    if (pos < 0 || pos > this.text.length) throw new RangeError(`offset ${pos} out of range`);
    let lo = 0, hi = this.starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.starts[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }
  lineStart(line: number): number {
    return this.starts[line];
  }
  /** Text of a line, without its trailing "\n". */
  lineText(line: number): string {
    const end = line + 1 < this.starts.length ? this.starts[line + 1] : this.text.length;
    return this.text.slice(this.starts[line], end);
  }
  toLineCol(pos: number): { line: number; col: number } {
    const line = this.lineAt(pos);
    return { line, col: pos - this.starts[line] + 1 };
  }
  fromLineCol(line: number, col: number): number {
    return this.starts[line] + col;
  }
}
