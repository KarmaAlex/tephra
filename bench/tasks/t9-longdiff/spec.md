`parseSyncConflict` in `src/vaultUtils.ts` does not behave as its doc comment says: it rejects valid Syncthing conflict names, and the `stamp` it returns has the wrong format. Fix that function only (do not touch anything else in the file).

Answer with a unified diff against `src/vaultUtils.ts`.
