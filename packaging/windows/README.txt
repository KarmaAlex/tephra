Obsidilight for Windows (x86_64)
================================

Portable build: no installer, nothing to install. Unzip anywhere and run obsidilight.exe.

- Double-click obsidilight.exe: a folder picker asks for your vault (any folder of Markdown notes,
  e.g. an Obsidian vault).
- Or from a terminal / shortcut:   obsidilight.exe "C:\path\to\vault"
                                   obsidilight.exe "C:\path\to\vault\note.md"

Requirements: Windows 10 (1903) or later, 64-bit, with a DirectX 11 capable GPU driver.

App data (file recovery snapshots) is kept in %LOCALAPPDATA%\obsidilight.
Spell checking: put hunspell dictionaries (en_US.aff + en_US.dic) in <vault>\.obsidilight\dictionaries
or point the DICPATH environment variable at a folder containing them.

Licenses: LICENSE.txt (obsidilight, MIT); the embedded UI fonts are under LICENSE-IBMPlexSans.txt
(SIL OFL 1.1) and LICENSE-DejaVu.txt.
