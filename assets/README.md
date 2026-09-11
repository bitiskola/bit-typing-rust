# Application artwork

Place the canonical application icon here as:

- `favico.png`

This is the single source used for the live app, title bar, Windows executable,
installer, uninstaller, shortcuts, Control Panel entry, Ubuntu application menu,
and dock. The build scripts derive the required Windows ICO and Linux icon sizes
from this PNG, so do not edit generated icon files separately.

## About-page profile images

Place the two profile images here using these exact filenames:

- `micr0softstore.png`
- `cacto.tsx.png`

Source runs load them directly from this folder. `bit-typing.spec` embeds every
PNG in this folder into compiled Windows and Linux builds. If either file is
missing or invalid, the About page displays a styled initials placeholder.
