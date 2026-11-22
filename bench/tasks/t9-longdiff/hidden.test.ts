import { test } from "node:test";
import assert from "node:assert/strict";
import { parseSyncConflict as p, fileKind, splitCsvRow } from "./src/vaultUtils.ts";
test("md", () => assert.deepEqual(p("Notes/a.sync-conflict-20250101-120000-ABCDEF1.md"), { original: "Notes/a.md", stamp: "20250101-120000", device: "ABCDEF1" }));
test("no ext", () => assert.deepEqual(p("Notes/README.sync-conflict-20250101-120000-ABCDEF1"), { original: "Notes/README", stamp: "20250101-120000", device: "ABCDEF1" }));
test("unicode", () => assert.deepEqual(p("Terzo anno/Borůvka.sync-conflict-20241231-235959-Z9Z9Z9Z.md"), { original: "Terzo anno/Borůvka.md", stamp: "20241231-235959", device: "Z9Z9Z9Z" }));
test("png", () => assert.equal(p("Images/Pasted image 20250101.sync-conflict-20250102-010203-AAAAAAA.png")?.original, "Images/Pasted image 20250101.png"));
test("reject", () => { assert.equal(p("Notes/a.md"), null); assert.equal(p("a.sync-conflict-2025-120000-ABCDEF1.md"), null); assert.equal(p("a.sync-conflict-20250101-120000-abcdef1.md"), null); });
test("rest untouched", () => { assert.equal(fileKind("Images/x.PNG"), "image"); assert.deepEqual(splitCsvRow('a, "b,c" ,d'), ["a", "b,c", "d"]); });
