# Embedded fonts

Embedded into the binary (`include_bytes!` in `src/fonts.rs`) so the app renders
identically on every machine, regardless of installed system fonts — GPUI on Linux
maps `.SystemUIFont` to the hardcoded family "IBM Plex Sans", which is only present
where texlive/ttf-ibm-plex happens to be installed.

| Family | Files | Used for |
|---|---|---|
| IBM Plex Sans (OFL 1.1, `LICENSE-IBMPlexSans.txt`) | Regular/Italic/Medium/MediumItalic/SemiBold/SemiBoldItalic/Bold/BoldItalic | UI + editor text (sans) |
| DejaVu Sans Mono (Bitstream Vera license, `LICENSE-DejaVu.txt`) | Regular/Bold/Oblique/BoldOblique | code blocks, mono (the gpui-component default mono on Linux) |
| DejaVu Sans | Regular/Bold | glyph fallback (arrows, symbols) via GPUI's fallback stack |

Both are unmodified copies of the Arch `ttf-ibm-plex` / `ttf-dejavu` packages
(/usr/share/fonts/TTF), so rendering on the dev machine is byte-identical to the
system-font rendering before this change.
