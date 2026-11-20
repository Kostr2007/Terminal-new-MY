# Changes

## 0.3.0-alpha.7

- Rebuild `myls` around connected folder and file sections, adaptive columns, and right-aligned metadata in the long view.
- Grow rounded section surfaces from their accent rails through native Kitty frames while keeping names as selectable text.
- Rework `myfiles` and `mycd` with a single-line path, one row per entry, and a rounded selection surface that follows keyboard focus in Kitty.

## 0.3.0-alpha.6

- Render `myls` with reusable Kitty badges, palette-colored Git statuses and a single native name reveal after writing the list.
- Share directory, file and link artwork across `myls`, `mytree` and the `myfiles` header.
- Show bounded PNG previews directly in the Kitty file browser and picker, with graphics scoped to the alternate screen.

## 0.3.0-alpha.5

- Give `myupdate` a palette-colored morphing shape and animated wave in Kitty.
- Add `tnmy progress` and `myprogress` for command activity and measured progress from stdin.
- Use in-place Kitty graphics for the measured bar and a text meter in other terminals.

## 0.3.0-alpha.4

- Stream tree entries without animation delays between rows.
- Round the Kitty branch connectors, animate their tips and icons, and sweep over names with native frames, including at the end of large trees.
- Use directory entry types without fetching unused file metadata.

## 0.3.0-alpha.3

- Slide and fade tree branches and filenames in place before handing off to the final scrollback output.

## 0.3.0-alpha.2

- Cascading tree rows and palette-colored icons with a one-shot Kitty animation.
- Static tree graphics with `--no-motion` and reduced-motion settings.

## 0.3.0-alpha.1

- Native Kitty image placements for `tree`, with a text fallback for redirected output.
- Kitty animation frames and window progress for `nh` operations.
- Private structured Nix logs, build totals and offline log replay.

## 0.2.0

- Shared Material surfaces, state chips, action hints and compact/comfortable density.
- Background directory/preview/Git workers with latest-request cancellation.
- Visible-row rendering, cached Unicode sort/search keys, bounded scrollable previews.
- Git porcelain metadata, directory aggregation, 2-second cache and 600 ms budget.
- NUL path picker, `mycd`, and preview scrolling with wrapped-line bounds.
- Palette-derived eza/bat/fzf/delta themes; isolated bat cache; myt apply hook.
- Accessible text fallback and contrast reporting, six palette fixtures and layout references.
- PTY coverage of 10,000 entries and fish directory picking; actual tool theme parsing.

## 0.1.0

Initial CLI, file list/browser, myt palette integration, nh wrapper and Nix modules.
