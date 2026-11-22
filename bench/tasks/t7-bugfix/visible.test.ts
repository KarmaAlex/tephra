import { test } from "node:test";
import assert from "node:assert/strict";
import { LineIndex } from "./src/lineIndex.ts";
test("toLineCol at second line", () => {
  const li = new LineIndex("ab\ncd");
  assert.deepEqual(li.toLineCol(3), { line: 1, col: 0 });
});
