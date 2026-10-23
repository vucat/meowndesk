# Native app icons

Generated from the original `src/assets/icons/meowndesk.svg` with the pinned
Tauri CLI (`npm run tauri icon -- src/assets/icons/meowndesk.svg --output src-tauri/icons`).
The desktop bundle uses 32/128/256-pixel PNGs and the multi-size Windows ICO.
Linux targets deb/AppImage; `tauri.windows.conf.json` selects NSIS on Windows.
The CLI's mobile/macOS outputs are not needed for this Windows/Linux application.
