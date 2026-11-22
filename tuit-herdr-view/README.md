# tuit-herdr-view

A herdr-app mod: tuit's commit list for the folder of the focused pane.

The list and its colours come from the tuit binary itself:

```sh
tuit --json --limit 200   # {root, branch, colors, cc, commits[]}
```

## Install

herdr-app only hot-reloads mods it can watch, so the app needs real files, not a
symlink to this folder:

```sh
mkdir -p ~/.config/herdr-app/mods/tuit-herdr-view
cp mod.json main.js ~/.config/herdr-app/mods/tuit-herdr-view/
```

After editing this folder, repeat the `cp` (saving reloads the mod live).
`gpui-kit.d.ts` and `jsconfig.json` are written by the app and gitignored.

## Layout

Add a block whose `content` is `{ "mod": "tuit-herdr-view" }` to a personal
layout in `~/.config/herdr-app/layouts/`.
