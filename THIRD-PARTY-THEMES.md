# Third-party themes

Eight of the eighty-one themes in Panel Platform are other people's colour
schemes — seven works, since Solarized supplies two. They are included because
they are what people already know these palettes as (renaming Dracula would
mostly stop anyone finding it), and each keeps its real name, its author and its
licence.

Everything here is a _palette_: a set of colour values, re-pointed at this
application's own tokens. No code, no assets and no trademarks are used, and
none of these projects has endorsed or is affiliated with Panel Platform.

The remaining seventy-three themes are original.

| Theme in this app               | Original work | Author                        | Licence                                           |
| ------------------------------- | ------------- | ----------------------------- | ------------------------------------------------- |
| Dracula                         | Dracula       | Zeno Rocha and contributors   | MIT                                               |
| Monokai                         | Monokai       | Wimer Hazenberg               | Original colour scheme — credited, not affiliated |
| One Dark                        | One Dark      | Atom contributors             | MIT                                               |
| Nord                            | Nord          | Sven Greb / Arctic Ice Studio | MIT                                               |
| Solarized Dark, Solarized Light | Solarized     | Ethan Schoonover              | MIT                                               |
| Tokyo Night                     | Tokyo Night   | enkia                         | MIT                                               |
| Catppuccin Mocha                | Catppuccin    | Catppuccin contributors       | MIT                                               |

- Dracula — https://github.com/dracula/dracula-theme
- Monokai — https://monokai.nl/
- One Dark — https://github.com/atom/atom
- Nord — https://www.nordtheme.com/
- Solarized — https://ethanschoonover.com/solarized/
- Tokyo Night — https://github.com/enkia/tokyo-night-vscode-theme
- Catppuccin — https://github.com/catppuccin/catppuccin

Two notes on accuracy, because "the Dracula theme" implies an exactness this
cannot have:

- **These palettes were drawn for a text editor, not an application.** An editor
  needs a background, a foreground and a dozen syntax colours; this application
  needs four surface layers, three text weights, borders and status colours.
  Where the original does not supply a value, it is mixed from the ones that
  are adjacent to it rather than invented.
- **Two values are lifted for readability.** Dracula's comment colour (`#6272a4`)
  measures under 4:1 against its background, which is fine for comments in a
  code buffer and not fine for secondary text throughout an interface. It is
  lifted along the same hue until it clears 4.5:1. Every theme in the
  application is held to that floor by a test.

## Names that were deliberately not used

Seven themes were requested under names belonging to products rather than to
palettes. They ship under descriptive names instead:

| Requested as       | Shipped as      |
| ------------------ | --------------- |
| VS Code Dark       | Editor Dark     |
| GitHub Dark        | Repo Dark       |
| Windows 95         | System 95       |
| Windows XP         | System XP       |
| Classic Mac        | Classic Desktop |
| Minecraft Inspired | Blockcraft      |
| Dark Souls         | Ashen           |

These are original palettes evoking a familiar look, which is a different thing
from using someone's trademark to describe your own product. It is the same
standard already applied to the Discord glyph in this repository, which was
redrawn rather than traced.
