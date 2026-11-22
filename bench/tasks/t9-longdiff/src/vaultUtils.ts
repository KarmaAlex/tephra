/* Assorted vault utilities. Generated for the model benchmark. */

/** Convert bytes to kib. */
export function bytesToKib(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("bytesToKib: value must be finite");
  return value / 1024;
}

/** Convert kib to mib. */
export function kibToMib(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("kibToMib: value must be finite");
  return value / 1024;
}

/** Convert mib to gib. */
export function mibToGib(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("mibToGib: value must be finite");
  return value / 1024;
}

/** Convert ms to s. */
export function msToS(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("msToS: value must be finite");
  return value / 1000;
}

/** Convert s to min. */
export function sToMin(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("sToMin: value must be finite");
  return value / 60;
}

/** Convert min to h. */
export function minToH(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("minToH: value must be finite");
  return value / 60;
}

/** Convert h to d. */
export function hToD(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("hToD: value must be finite");
  return value / 24;
}

/** Convert mm to cm. */
export function mmToCm(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("mmToCm: value must be finite");
  return value / 10;
}

/** Convert cm to m. */
export function cmToM(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("cmToM: value must be finite");
  return value / 100;
}

/** Convert pt to px. */
export function ptToPx(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("ptToPx: value must be finite");
  return value / 1.3333333333333333;
}

/** Convert px to rem. */
export function pxToRem(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("pxToRem: value must be finite");
  return value / 16.0;
}

/** Convert deg to rad. */
export function degToRad(value: number): number {
  if (!Number.isFinite(value)) throw new TypeError("degToRad: value must be finite");
  return value / 57.29577951308232;
}

/** Validate a `title` frontmatter value. Returns an error message or null. */
export function validateTitle(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "title must be a string";
  const v = raw.trim();
  if (v.length === 0) return "title must not be empty";
  if (v.length > 80) return "title is longer than 80 characters";
  if (/[\u0000-\u001f]/.test(v)) return "title contains control characters";
  return null;
}

/** Validate a `alias` frontmatter value. Returns an error message or null. */
export function validateAlias(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "alias must be a string";
  const v = raw.trim();
  if (v.length === 0) return "alias must not be empty";
  if (v.length > 64) return "alias is longer than 64 characters";
  if (/[\u0000-\u001f]/.test(v)) return "alias contains control characters";
  return null;
}

/** Validate a `tag` frontmatter value. Returns an error message or null. */
export function validateTag(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "tag must be a string";
  const v = raw.trim();
  if (v.length === 0) return "tag must not be empty";
  if (v.length > 120) return "tag is longer than 120 characters";
  if (/[\u0000-\u001f]/.test(v)) return "tag contains control characters";
  return null;
}

/** Validate a `cssclass` frontmatter value. Returns an error message or null. */
export function validateCssclass(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "cssclass must be a string";
  const v = raw.trim();
  if (v.length === 0) return "cssclass must not be empty";
  if (v.length > 32) return "cssclass is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "cssclass contains control characters";
  return null;
}

/** Validate a `author` frontmatter value. Returns an error message or null. */
export function validateAuthor(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "author must be a string";
  const v = raw.trim();
  if (v.length === 0) return "author must not be empty";
  if (v.length > 32) return "author is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "author contains control characters";
  return null;
}

/** Validate a `course` frontmatter value. Returns an error message or null. */
export function validateCourse(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "course must be a string";
  const v = raw.trim();
  if (v.length === 0) return "course must not be empty";
  if (v.length > 200) return "course is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "course contains control characters";
  return null;
}

/** Validate a `semester` frontmatter value. Returns an error message or null. */
export function validateSemester(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "semester must be a string";
  const v = raw.trim();
  if (v.length === 0) return "semester must not be empty";
  if (v.length > 32) return "semester is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "semester contains control characters";
  return null;
}

/** Validate a `professor` frontmatter value. Returns an error message or null. */
export function validateProfessor(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "professor must be a string";
  const v = raw.trim();
  if (v.length === 0) return "professor must not be empty";
  if (v.length > 80) return "professor is longer than 80 characters";
  if (/[\u0000-\u001f]/.test(v)) return "professor contains control characters";
  return null;
}

/** Validate a `room` frontmatter value. Returns an error message or null. */
export function validateRoom(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "room must be a string";
  const v = raw.trim();
  if (v.length === 0) return "room must not be empty";
  if (v.length > 200) return "room is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "room contains control characters";
  return null;
}

/** Validate a `exam` frontmatter value. Returns an error message or null. */
export function validateExam(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "exam must be a string";
  const v = raw.trim();
  if (v.length === 0) return "exam must not be empty";
  if (v.length > 32) return "exam is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "exam contains control characters";
  return null;
}

/** Validate a `topic` frontmatter value. Returns an error message or null. */
export function validateTopic(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "topic must be a string";
  const v = raw.trim();
  if (v.length === 0) return "topic must not be empty";
  if (v.length > 200) return "topic is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "topic contains control characters";
  return null;
}

/** Validate a `chapter` frontmatter value. Returns an error message or null. */
export function validateChapter(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "chapter must be a string";
  const v = raw.trim();
  if (v.length === 0) return "chapter must not be empty";
  if (v.length > 64) return "chapter is longer than 64 characters";
  if (/[\u0000-\u001f]/.test(v)) return "chapter contains control characters";
  return null;
}

/** Validate a `section` frontmatter value. Returns an error message or null. */
export function validateSection(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "section must be a string";
  const v = raw.trim();
  if (v.length === 0) return "section must not be empty";
  if (v.length > 32) return "section is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "section contains control characters";
  return null;
}

/** Validate a `language` frontmatter value. Returns an error message or null. */
export function validateLanguage(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "language must be a string";
  const v = raw.trim();
  if (v.length === 0) return "language must not be empty";
  if (v.length > 32) return "language is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "language contains control characters";
  return null;
}

/** Validate a `status` frontmatter value. Returns an error message or null. */
export function validateStatus(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "status must be a string";
  const v = raw.trim();
  if (v.length === 0) return "status must not be empty";
  if (v.length > 120) return "status is longer than 120 characters";
  if (/[\u0000-\u001f]/.test(v)) return "status contains control characters";
  return null;
}

/** Validate a `location` frontmatter value. Returns an error message or null. */
export function validateLocation(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "location must be a string";
  const v = raw.trim();
  if (v.length === 0) return "location must not be empty";
  if (v.length > 120) return "location is longer than 120 characters";
  if (/[\u0000-\u001f]/.test(v)) return "location contains control characters";
  return null;
}

/** Validate a `credits` frontmatter value. Returns an error message or null. */
export function validateCredits(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "credits must be a string";
  const v = raw.trim();
  if (v.length === 0) return "credits must not be empty";
  if (v.length > 32) return "credits is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "credits contains control characters";
  return null;
}

/** Validate a `grade` frontmatter value. Returns an error message or null. */
export function validateGrade(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "grade must be a string";
  const v = raw.trim();
  if (v.length === 0) return "grade must not be empty";
  if (v.length > 64) return "grade is longer than 64 characters";
  if (/[\u0000-\u001f]/.test(v)) return "grade contains control characters";
  return null;
}

/** Validate a `year` frontmatter value. Returns an error message or null. */
export function validateYear(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "year must be a string";
  const v = raw.trim();
  if (v.length === 0) return "year must not be empty";
  if (v.length > 32) return "year is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "year contains control characters";
  return null;
}

/** Validate a `teacher` frontmatter value. Returns an error message or null. */
export function validateTeacher(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "teacher must be a string";
  const v = raw.trim();
  if (v.length === 0) return "teacher must not be empty";
  if (v.length > 200) return "teacher is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "teacher contains control characters";
  return null;
}

/** Validate a `lab` frontmatter value. Returns an error message or null. */
export function validateLab(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "lab must be a string";
  const v = raw.trim();
  if (v.length === 0) return "lab must not be empty";
  if (v.length > 120) return "lab is longer than 120 characters";
  if (/[\u0000-\u001f]/.test(v)) return "lab contains control characters";
  return null;
}

/** Validate a `project` frontmatter value. Returns an error message or null. */
export function validateProject(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "project must be a string";
  const v = raw.trim();
  if (v.length === 0) return "project must not be empty";
  if (v.length > 32) return "project is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "project contains control characters";
  return null;
}

/** Validate a `deadline` frontmatter value. Returns an error message or null. */
export function validateDeadline(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "deadline must be a string";
  const v = raw.trim();
  if (v.length === 0) return "deadline must not be empty";
  if (v.length > 200) return "deadline is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "deadline contains control characters";
  return null;
}

/** Validate a `priority` frontmatter value. Returns an error message or null. */
export function validatePriority(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "priority must be a string";
  const v = raw.trim();
  if (v.length === 0) return "priority must not be empty";
  if (v.length > 32) return "priority is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "priority contains control characters";
  return null;
}

/** Validate a `summary` frontmatter value. Returns an error message or null. */
export function validateSummary(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "summary must be a string";
  const v = raw.trim();
  if (v.length === 0) return "summary must not be empty";
  if (v.length > 64) return "summary is longer than 64 characters";
  if (/[\u0000-\u001f]/.test(v)) return "summary contains control characters";
  return null;
}

/** Validate a `source` frontmatter value. Returns an error message or null. */
export function validateSource(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "source must be a string";
  const v = raw.trim();
  if (v.length === 0) return "source must not be empty";
  if (v.length > 200) return "source is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "source contains control characters";
  return null;
}

/** Validate a `license` frontmatter value. Returns an error message or null. */
export function validateLicense(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "license must be a string";
  const v = raw.trim();
  if (v.length === 0) return "license must not be empty";
  if (v.length > 32) return "license is longer than 32 characters";
  if (/[\u0000-\u001f]/.test(v)) return "license contains control characters";
  return null;
}

/** Validate a `version` frontmatter value. Returns an error message or null. */
export function validateVersion(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "version must be a string";
  const v = raw.trim();
  if (v.length === 0) return "version must not be empty";
  if (v.length > 200) return "version is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "version contains control characters";
  return null;
}

/** Validate a `category` frontmatter value. Returns an error message or null. */
export function validateCategory(raw: unknown): string | null {
  if (raw === undefined || raw === null) return null;
  if (typeof raw !== "string") return "category must be a string";
  const v = raw.trim();
  if (v.length === 0) return "category must not be empty";
  if (v.length > 200) return "category is longer than 200 characters";
  if (/[\u0000-\u001f]/.test(v)) return "category contains control characters";
  return null;
}

export type FileKind = "image" | "document" | "note" | "media" | "data" | "other";

export interface SyncConflict {
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
  const m = /^(.*)\.sync-conflict-(\d{8})-(\d{6})-([A-Z0-9]{6})(\..*)?$/.exec(path);
  if (!m) return null;
  return { original: m[1] + m[5], stamp: m[2] + m[3], device: m[4] };
}

const KIND_BY_EXT: Record<string, FileKind> = {
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  svg: "image",
  webp: "image",
  pdf: "document",
  md: "note",
  canvas: "note",
  base: "note",
  mp3: "media",
  mp4: "media",
  csv: "data",
  json: "data",
};

/** Classify a vault path by extension. */
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
  return parts.filter((p) => p.length > 0).join("/").replace(/\/{2,}/g, "/");
}

/** Split a csvRow into trimmed cells, honouring double quotes. */
export function splitCsvRow(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (quoted && line[i + 1] === '"') { cur += '"'; i++; } else quoted = !quoted;
    } else if (ch === "," && !quoted) {
      cells.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  cells.push(cur.trim());
  return cells;
}

/** Split a tsvRow into trimmed cells, honouring double quotes. */
export function splitTsvRow(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (quoted && line[i + 1] === '"') { cur += '"'; i++; } else quoted = !quoted;
    } else if (ch === "\t" && !quoted) {
      cells.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  cells.push(cur.trim());
  return cells;
}

/** Split a pipeRow into trimmed cells, honouring double quotes. */
export function splitPipeRow(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (quoted && line[i + 1] === '"') { cur += '"'; i++; } else quoted = !quoted;
    } else if (ch === "|" && !quoted) {
      cells.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  cells.push(cur.trim());
  return cells;
}

/** Split a semicolonRow into trimmed cells, honouring double quotes. */
export function splitSemicolonRow(line: string): string[] {
  const cells: string[] = [];
  let cur = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') {
      if (quoted && line[i + 1] === '"') { cur += '"'; i++; } else quoted = !quoted;
    } else if (ch === ";" && !quoted) {
      cells.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  cells.push(cur.trim());
  return cells;
}

/** Format a date as DD-MM-YY. */
export function formatDdMmYy_0(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "DD-MM-YY".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Format a date as YYYY-MM-DD. */
export function formatYyyyMmDd_1(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "YYYY-MM-DD".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Format a date as DD/MM/YYYY. */
export function formatDdMmYyyy_2(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "DD/MM/YYYY".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Format a date as YYYYMMDD. */
export function formatYyyymmdd_3(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "YYYYMMDD".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Format a date as DD.MM.YYYY. */
export function formatDdMmYyyy_4(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "DD.MM.YYYY".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Format a date as MM-DD-YYYY. */
export function formatMmDdYyyy_5(d: Date): string {
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = String(d.getFullYear());
  const yy = yyyy.slice(-2);
  return "MM-DD-YYYY".replace("YYYY", yyyy).replace("YY", yy).replace("MM", mm).replace("DD", dd);
}

/** Numeric helper #0. */
export function snap0(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #1. */
export function clamp1(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #2. */
export function lerp2(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #3. */
export function clamp3(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}

/** Numeric helper #4. */
export function wrap4(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #5. */
export function lerp5(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #6. */
export function roundTo6(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #7. */
export function snap7(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}

/** Numeric helper #8. */
export function lerp8(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #9. */
export function wrap9(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #10. */
export function clamp10(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #11. */
export function wrap11(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}

/** Numeric helper #12. */
export function roundTo12(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #13. */
export function wrap13(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #14. */
export function lerp14(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #15. */
export function clamp15(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}

/** Numeric helper #16. */
export function wrap16(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #17. */
export function wrap17(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #18. */
export function lerp18(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #19. */
export function roundTo19(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}

/** Numeric helper #20. */
export function clamp20(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1) / 1;
}

/** Numeric helper #21. */
export function wrap21(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 10) / 10;
}

/** Numeric helper #22. */
export function clamp22(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 100) / 100;
}

/** Numeric helper #23. */
export function wrap23(x: number, a: number, b: number): number {
  if (a > b) [a, b] = [b, a];
  const t = Math.min(Math.max(x, a), b);
  return Math.round(t * 1000) / 1000;
}
