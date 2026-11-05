# What Layerfold changes in EffectCraft

Layerfold is based on [EffectCraft](https://github.com/storytold/effectcraft) 0.21.0, tag `v0.21.0`,
commit `7870b5916ae0749d497d880989683bf2946d6c8b`. This file lists how it differs. It also serves as
the notice of modified files that the Apache-2.0 license asks for. To see every changed line, compare
this repository with that tag. Most of the changes are in `crates/ui-egui/`, `crates/engine/`,
`crates/project/` and `editor/`.

## Layer groups

- Layers can be grouped, and groups can be nested. The commands are `layer.group`, `layer.ungroup`,
  `layer.groupAdd`, `layer.groupRemove` and `layer.groupUpdate`, and they work from scripts as well.
- Each group has Video, Audio and Lock switches that act on everything inside it. A group's Video
  works like the eye of each layer inside: the layers leave the picture, but effects can still read
  them and a track matte inside still works.
- Layers go in and out of groups by dragging, and whole groups can be moved the same way.
- Each group's row has a read-only hatched bar: the union of its layers' spans, with thin lines across
  the gaps. Clicking it selects the group's layers, and a double click collapses or expands the group.
- A locked group protects its layers from commands as well as from the mouse: paste, delete,
  precompose, analysis jobs (tracking, Roto Brush, Warp Stabilizer and others) and layers created from
  other layers. A layer can't be removed while a layer in a locked group depends on it through a parent,
  a matte or an expression.
- Rendering, solo, export (layered frames, Lottie, FCP XML, FCPXML, OTIO) and the `active`, `hasVideo`
  and `audioActive` values in expressions and scripts take groups into account.
- Precompose takes layers from one group level only, and the new layer takes their place in the group.

## Timeline editing

- Alt+drag on a clip slips the video inside it (`layer.slip` with the new `holdKeys` parameter). The
  clip and its keyframes stay, and layer markers move with the video. Without `holdKeys` the command
  behaves as in After Effects, for scripts.
- Clip edges stick to the start and end of the source within 8 pixels and go past them into a frozen
  frame if you keep pulling. The bar that shows the source length is thicker.
- Layer ▸ Time ▸ Trim to Source (`layer.trimToSource`) stretches a clip to the whole source.
- Video and image layers show frames on their tracks (`panels/filmstrip.rs`). They are off for nested
  compositions and layers with Time Remap. The header button `timeline.switch.layerFrames` turns them off
  for the timeline, and the layer switch `frames` turns them off for one layer. The second is saved in
  the project and is part of Undo.
- Shift snaps keyframes and layer edges to the playhead, other keyframes, layer edges and group edges
  within 8 pixels, with a line and a label for the target. In the graph editor it works with the
  Snap button off.
- The middle mouse button pans the timeline through time and through the rows.
- Alt+click picks the active layer of a selection, in the timeline and in the viewer. A new group
  gathers around it (`layer.setActive`).
- Group rows have the same separator line as layer rows.

## Playback

- Audio switches and solo can be changed during playback.

## Windows

- An MSI installer (`editor/packaging/windows/`, `editor/scripts/package-msi.ps1`) that installs beside
  the official EffectCraft. It isn't code-signed.
- A build setup that doesn't need Visual Studio: a local Rust and LLVM-MinGW in `editor/.tools/`.
- Fixes for failures specific to this toolchain: posterize rounding in `crates/effects/src/misc.rs`,
  which made CPU and GPU results differ, and path and rounding assumptions in tests.
- Font fallback for Arabic, Hebrew, Indic, Thai and CJK text no longer lets a loaded third-party font
  replace the system family (`crates/text/src/fonts.rs`).

## Name and identity

- The product is called Layerfold, with its own icon, window title and settings folder
  (`%APPDATA%\Layerfold`). On first run it copies settings from `%APPDATA%\EffectCraft` once.
- ArtCraft logos and brand files are removed, as their terms require for modified versions.
- Removed: the update check, the links to Discord, the ArtCraft site and sibling apps, and upstream's
  release, packaging and CI files. Help and About link to this repository and to the original project.
- Internal names stay: crates are `effectcraft-*`, programs are `effectcraft.exe` and
  `effectcraft-cli.exe`, and projects are `.ecproj`, so EffectCraft projects open here and later
  EffectCraft versions can be merged.

## Security

These close some gaps inherited from EffectCraft, not all of them. **Open projects and run scripts only
from people you trust:** a project can still name a file or an output path outside these checks, and
a script can still write and read files through other commands, even with its permission off.

- Proxy paths stored in a project, and footage paths in an imported template or timeline, now follow
  the same rule as a project's own footage.
- With Allow Scripts to Write Files and Access Network off, a script can no longer write a file it
  names through commands such as `file.saveCopy`. It can still save a project as `.ecproj`.
- The control channel (`--control`) now asks for a token that the app saves in its settings folder for
  your user, and closes a connection that doesn't send it within 10 seconds. See
  `docs/control-protocol.md`.
- `editor/scripts/setup.ps1` checks the hash of the Rust installer it downloads.

## Added files

- `editor/` holds our build scripts, installer and demo filming, some of them in Russian.
- `docs/images/demo/` holds the animations in the README. `editor/scripts/demo/` films them from
  synthetic clips with the `snapshot` example, which gained step-by-step pointer gestures for this.

## Known limits

- Tested on one Windows computer, mostly with synthetic input. The installer hasn't been tried on
  another computer. Linux, macOS and the web version are not tested here.
- Keyframes made by tracking stay in place during a slip, so tracking has to be repeated afterwards.
- All expressions share one JavaScript environment (from `crates/expr`). An expression that deliberately
  redefines a shared function can get around the locked group check. The result is an allowed delete,
  which Undo reverses.
- The interface's Russian translation is incomplete.
- Building the MSI needs a licence bundle in `editor/output/share-windows/`, which isn't in this
  repository and no script here creates. The program itself builds without it.
