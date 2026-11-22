import { test } from "node:test";
import assert from "node:assert/strict";
import { slugify, wordCount } from "./src/index.ts";
test("slug accents", () => assert.equal(slugify("Calcolabilità e Complessità"), "calcolabilita-e-complessita"));
test("slug boruvka", () => assert.equal(slugify("Algoritmo di Borůvka!"), "algoritmo-di-boruvka"));
test("slug trim", () => assert.equal(slugify("  --Hello, World--  "), "hello-world"));
test("slug digits", () => assert.equal(slugify("Esame 2025/01"), "esame-2025-01"));
test("slug empty", () => assert.equal(slugify("!!!"), ""));
test("wc", () => { assert.equal(wordCount("uno due  tre\nquattro"), 4); assert.equal(wordCount("   "), 0); assert.equal(wordCount(""), 0); assert.equal(wordCount("$x^2$ è"), 2); });
