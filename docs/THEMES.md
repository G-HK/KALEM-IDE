# Themes

Kalem has two basic palettes, **Light** and **Dark**, in the built-in theme **Kalem** (`themes/kalem.toml`).
The Light palette is the Black Panda palette. Every theme, built in or yours, carries its own `[light]`
and `[dark]` palette, and **View > Appearance** picks Light, Dark or System.

## Adding your own theme

1. **View > Theme > Open Themes Folder** (creates the folder and a `_template.toml.example`).
2. Copy the template to `my-theme.toml`, change `name`, and edit the colours you want.
3. **View > Theme > Reload Themes**, then pick it under **View > Theme**.

Colours you leave out fall back to "Kalem". Mistakes (unknown colour name, not `#RRGGBB`) do not stop the
theme loading; the first problem is shown in the toolbar.

The themes folder is `%APPDATA%\kalem\themes` on Windows, `~/Library/Application Support/kalem/themes` on
macOS and `~/.config/kalem/themes` on Linux (`KALEM_CONFIG_DIR` overrides the `kalem` folder). The choice
is saved next to it in `settings.toml`.

## Colours

| Token | Used for |
|---|---|
| `background`, `panel`, `foreground`, `muted_foreground`, `border` | Window, side panels, text, secondary text, lines |
| `list_active` | Selected rows and tabs |
| `accent` | Focused panel border, active tab, links, caret |
| `success`, `warning`, `danger` | Added, modified and deleted files; pass and fail |
| `folder`, `project`, `solution`, `lock` | Solution Explorer icons |

File type badges (the `YML` on a `.yml` file) are recoloured per theme in `[light.badges]` / `[dark.badges]`,
keyed by the badge label in lower case, for example `yml = "#ff0000"`.

```toml
name = "Ocean"
[dark]
background = "#0b1d2a"
accent = "#35c2ff"
[dark.badges]
rs = "#ffb86c"
```

Editor syntax colours are not themeable yet.
