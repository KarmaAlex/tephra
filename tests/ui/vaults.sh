#!/usr/bin/env bash
# Vaults: started without an argument and no last vault, the app shows the welcome page;
# "Create new vault" there creates the folder (+ .obsidian/) and opens it in that process;
# the next start without an argument reopens it; a recent vault on the welcome page opens;
# from a vault window, the palette command, the File menu and the vault manager open other
# vaults in a new window (printed as NEW-WINDOW in scripted runs). SHOT=<dir> saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT
V="$ROOT/vaults"
mkdir -p "$V/Existing/.obsidian" "$ROOT/cfg"
printf '# E\n' > "$V/Existing/E.md"
export TEPHRA_CONFIG_DIR="$ROOT/cfg"
fail() { echo "ui-vaults: $1"; exit 1; }
run() { TEPHRA_SCRIPT="$ROOT/s.script" timeout 40 target/release/tephra "$@" | grep -E "^(ROOT|NEW-WINDOW|CLICKK|NO-EDITOR)" || true; }

# 1. the last vault is gone: welcome page; create a vault there
printf '{"last": "%s", "recent": ["%s", "%s"]}' "$V/Gone" "$V/Gone" "$V/Existing" > "$ROOT/cfg/vaults.json"
cat > "$ROOT/s.script" <<SCRIPT
wait 600
ws-root
${SHOT:+shot $SHOT/welcome.png}
clickk welcome:create
wait 500
type New Vault
${SHOT:+shot $SHOT/create.png}
key enter
wait 1500
ws-root
quit
SCRIPT
OUT=$(run)
echo "$OUT"
[ "$OUT" = "ROOT none
ROOT $V/New Vault" ] || fail "welcome -> create: $OUT"
[ -d "$V/New Vault/.obsidian" ] || fail "the vault folder or its .obsidian/ is missing"
grep -qF "\"last\": \"$V/New Vault\"" "$ROOT/cfg/vaults.json" || fail "last vault not saved: $(cat "$ROOT/cfg/vaults.json")"

# 2. started again without an argument: the last vault; the palette command and the File
#    menu create vaults that open in new windows
cat > "$ROOT/s.script" <<SCRIPT
wait 600
ws-root
key ctrl-p
type create new vault
key enter
wait 500
type Second
key enter
wait 800
clickk ribbon:file-menu
wait 400
${SHOT:+shot $SHOT/file-menu.png}
key down 2
key enter
wait 500
type Third
key enter
wait 800
quit
SCRIPT
OUT=$(run)
echo "$OUT"
[ "$OUT" = "ROOT $V/New Vault
NEW-WINDOW $V/Second
NEW-WINDOW $V/Third" ] || fail "startup / palette / file menu: $OUT"
[ -d "$V/Second/.obsidian" ] && [ -d "$V/Third/.obsidian" ] || fail "Second/Third not created"

# 3. the welcome page's recent list opens a vault; from a vault window, the vault manager
#    opens another vault in a new window, and the current one is not opened twice
cat > "$ROOT/s.script" <<SCRIPT
wait 600
clickk welcome:recent:Existing
wait 1500
ws-root
key ctrl-p
type open vault manager
key enter
wait 800
ws-root
clickk welcome:recent:Existing
wait 300
clickk welcome:recent:Second
wait 500
quit
SCRIPT
printf '{"last": "%s", "recent": ["%s", "%s", "%s"]}' "$V/Gone" "$V/Gone" "$V/Existing" "$V/Second" > "$ROOT/cfg/vaults.json"
OUT=$(run)
echo "$OUT"
[ "$OUT" = "ROOT $V/Existing
ROOT none
NEW-WINDOW $V/Second" ] || fail "recent list / vault manager: $OUT"
echo "ui-vaults: OK"
