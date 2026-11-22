export function wordCount(s: string): number { return (s.match(/\S+/g) ?? []).length; }
