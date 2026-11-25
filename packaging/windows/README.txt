Tephra for Windows (x86_64)
================================

Portable build: no installer, nothing to install. Unzip anywhere and run tephra.exe.

- Double-click tephra.exe: the vault you opened last opens again. The first time, a welcome
  page lets you create a vault or open a folder (any folder of Markdown notes, e.g. an Obsidian
  vault). The menu at the top left (File) opens other vaults, each in its own window.
- Or from a terminal / shortcut:   tephra.exe "C:\path\to\vault"
                                   tephra.exe "C:\path\to\vault\note.md"

Requirements: Windows 10 (1903) or later, 64-bit, with a DirectX 11 capable GPU driver.

App data (file recovery snapshots) is kept in %LOCALAPPDATA%\tephra; the list of recent
vaults in %APPDATA%\tephra\vaults.json.
Spell checking: put hunspell dictionaries (en_US.aff + en_US.dic) in <vault>\.tephra\dictionaries
or point the DICPATH environment variable at a folder containing them.

Licenses: LICENSE.txt (tephra, MIT); the embedded UI fonts are under LICENSE-IBMPlexSans.txt
(SIL OFL 1.1) and LICENSE-DejaVu.txt.
