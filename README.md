# KROMA

DaVinci's colour page, rebuilt for photographs, with a stacked reorderable
inspector instead of a node graph. Free and open source, MIT or Apache-2.0.

**Status: the colour page works.** Thirty effects run on the GPU, eleven of
them pinned as fixed panels the way Lightroom lays them out, the rest a
reorderable stack with per-row blend, blend mode and enable. Curves — four tone
and six secondaries — the Colour Warper's three views, wheels, scopes, crop and
straighten, a filmstrip, batch export. Isolation (qualifier, power windows) is
next.

A photograph that carries an ICC profile is read as the space that profile
describes, so a Display P3 file from a phone renders as Display P3 rather than
being flattened into sRGB. Unrecognised profiles change nothing, and the File
page lets you say what a file is when it does not say for itself. Exports carry
a profile of their own — an APP2 segment in a JPEG, an `iCCP` chunk in a PNG —
so what comes out says what it is rather than being read as sRGB by everything
and hoping.

Effect parameters follow Resolve's own — names, ranges and defaults read off
Resolve's inspector and recorded in
[docs/resolve-parameters.md](docs/resolve-parameters.md), which marks each value
confirmed against a screenshot or inferred.

## Running it

```bash
cargo run -p pe-windows --release -- photo.jpg
```

With no argument it shows a built-in test chart.

Add effects from the menu at the top of the inspector, reorder them with the
arrows, and drag any slider. The **passes** counter in the toolbar is the number
to watch: with a nine-row stack, dragging the deepest slider should read `1`.
That is the stage cache re-running only what changed, and it is why the
application does not get slower as you do more to an image.

## Autosave

Every edit is written out about a second after you stop moving, and read back
when the photograph is next opened. Closing the window is not a decision:
close mid-grade, come back tomorrow, carry on.

It is kept **with the application**, under `%APPDATA%\Kroma\edits`, not beside
your photographs. A `.peproj` sidecar is something you ask for; this happens
whether you asked or not, and an application that sprinkles files through
somebody's photo library uninvited is doing something it was not invited to
do. A photo directory that has never been written to stays that way.

The two are not rivals. A sidecar is a decision — *this* is the edit, keep it,
move it with the photograph. The autosave is just where you happened to stop,
so it is the one that wins when a photograph is opened, and **Load edit** is
the explicit way to pull a sidecar back over the top.

**Revert** throws both the edit and the saved work away. An edit that comes
back every time you open a photograph with no way to be rid of it is not a
convenience; it is a photograph you can no longer see.

## Your originals are never written to

The application does not modify the photograph you opened. It cannot: every
write is checked against every file in the open set first, and a collision is
refused rather than resolved.

Exports are named after the original with `_KROMA` on the end —
`DJI_0001.JPG` becomes `DJI_0001_KROMA.jpg`. That suffix is not decoration. An
export named after its source, in the folder its source lives in, *is* its
source on a filesystem that ignores case, and Windows ignores case. The naming
and the check are two separate defences on purpose: a scheme that happens to
differ is not a guarantee — and it would not hold anyway once you can export a
PNG of a PNG, which the File page lets you do in one click.

Format and JPEG quality live on the File page rather than behind a dialog.
They are settings, not a question: a dialog asks the same thing every time and
gets the same answer every time, and the panel keeps yours between sessions.
JPEG, 8-bit PNG and 16-bit PNG. The last one is where a wide working space
stops being theoretical — a gradient pushed about by a dozen rows holds more
distinct values than 8 bits can name, and it is the only way out that keeps
them.

The only other thing written beside a photograph is its `.peproj` sidecar,
which holds the edit and appears only when you ask for it with Save edit or
Save all. Nothing at all is written when the window closes.

## Keys

| | |
|---|---|
| `Ctrl+O` / `Ctrl+Shift+O` | open a photograph / a folder |
| `Ctrl+S` | save the edit as a `.peproj` sidecar |
| `Ctrl+E` | export a JPEG |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo, with slider drags collapsed into one step |
| `←` / `→` | previous / next photograph in the set |
| `F` / `S` / `C` | filmstrip / scopes / crop |
| `Shift+D` | bypass the whole stack |
| scroll | zoom, anchored under the cursor |
| double-click | fit to the window |

They all stand down while you are typing into a parameter's number box, so
correcting a value with the arrow keys does not change which photograph you
are looking at.

## Layout

```
crates/
  pe-color/    colour spaces, transfer functions, the two-space pipeline
  pe-core/     document model: stack, parameters, history
  pe-effects/  effect registry — params, shaders, working-space declarations
  pe-render/   wgpu device, 16f textures, the stage cache
  pe-scopes/   waveform / parade / vectorscope / histogram (CPU reference)
  pe-io/       image decode and encode
  pe-session/  the workflow layer: the open photograph, autosave, export rules
  pe-ffi/      C ABI surface for Swift
shaders/       .wgsl, shared by every platform
apps/
  windows/     Rust shell — contains no image processing
  apple/       macOS shell, and the Swift it shares with iOS later
tests/golden/  reference renders, diffed by CI
docs/          the two decision records worth reading first
```

### The firewall rule

`apps/windows` contains **no image processing whatsoever**. Its entire
vocabulary is: read the stack, mutate a parameter, ask `pe-render` for a
texture, draw it. The day a convenience function that touches pixels appears in
a UI crate is the day the Mac port silently becomes a rewrite.

The rule extends downward as well. `pe-session` holds the things that are
neither pixels nor interface — which photograph is open, where work in progress
is kept, and what may be written where. "Never write over an original" is not a
Windows rule, and a rule implemented twice is a rule that will differ.

## Two ideas everything else follows from

**The two-space rule.** Every effect declares whether it simulates *light*
(ACEScg, linear) or shapes *perception* (ACEScct, log), and the renderer
inserts the transform. No effect converts its own input. See
[docs/color-pipeline.md](docs/color-pipeline.md).

**Every stack row is a node in disguise.** Each row carries its own `enabled`,
`opacity`, `blend` and `key`, mirroring a Resolve node's anatomy. So "grain at
40% in Screen mode, sky only" is three fields that already exist rather than a
feature built into each effect. See
[docs/document-format.md](docs/document-format.md).

## Testing

```bash
cargo test --workspace
```

Golden references live in `tests/golden/refs/` and are committed. When output
changes, the failure writes the actual render and an amplified difference map
to `tests/golden/out/`. **Look at the diff**, then if the change is intended:

```bash
PE_UPDATE_GOLDEN=1 cargo test -p pe-golden
```

A golden test blindly regenerated is a golden test deleted.

## Milestones

| | | |
|---|---|---|
| **M0** | Foundations | ✅ complete |
| **M1** | The engine, proven — nine effects, stage cache, export | ✅ complete |
| — | Resolve film effects — Split Tone, Dehaze, Bloom, Film Damage | ✅ |
| M2 | The Colour Page — real UI, scopes, wheels, curve editor | |
| M3 | Isolation — qualifier, power windows, masks | |
| M4 | Grading workflow — stills gallery, versions, compare | |
| M5 | RAW | |
| M6 | macOS | in progress — five of eight control kinds, every effect reachable |

## Known constraints

**Pinned to wgpu 27 and egui 0.33.** Two independent ceilings meet here:

- wgpu 30's DX12 backend needs `windows` 0.62 while the newest published
  `gpu-allocator` (0.28) is built against 0.58, so it does not compile on
  Windows at all.
- egui 0.35 removed `TopBottomPanel`/`SidePanel` and replaced `eframe::App`'s
  `update` with `ui`. Since the M1 interface is disposable, following that
  rewrite now would be work thrown away at M2.

egui 0.33 pairs with wgpu 27, which is what keeps a single wgpu in the graph.
Two versions in one build is not merely untidy — `Device` and `Queue` from
different versions are different types, and the duplicate also broke `naga`
through a `codespan-reporting` feature clash.

**A 24MP export needs about 576 MB of VRAM** — two working textures plus source
and output. Flat in stack depth, but worth knowing before a batch run on a
small card.

**Halation is a single-pass approximation** at M1. A separable multi-pass blur
is M2; the current version shows its seams at large radii.

**Building the Apple side from a network share does not work.** Cargo's
defaults assume local disk: incremental compilation cannot take its locks over
SMB or NFS, and a half-written artefact fails to load with an error that blames
the dependency rather than the disc that dropped it. The fix is
`CARGO_INCREMENTAL=0` and a `CARGO_TARGET_DIR` on local disk —
`build-engine.sh` already honours the latter, writing the universal library
back inside the repository so `project.yml` stays machine-independent.

## Licence

MIT or Apache-2.0, at your option. `LICENSE-MIT` is in the repository;
`LICENSE-APACHE` should hold the official text from
<https://www.apache.org/licenses/LICENSE-2.0.txt> — dropped in verbatim rather
than retyped, because a licence that differs from the canonical text by a word
is a licence nobody can rely on.
