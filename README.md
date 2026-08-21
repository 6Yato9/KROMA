# Photo Editor

DaVinci's colour page, rebuilt for photographs, with a stacked reorderable
inspector instead of a node graph.

**Status: M1 complete, plus the Resolve film effects.** Thirteen effects run on
the GPU, the stack is reorderable with per-row Blend, blend mode and enable, and
export writes a full-resolution JPEG. Scopes, wheels and curve editing are M2.

Effect parameters follow Resolve's own — names, ranges and defaults researched
from the Resolve 20 manuals and recorded in
[docs/resolve-parameters.md](docs/resolve-parameters.md), which marks each value
confirmed or inferred.

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

`Shift+D` bypasses the whole stack. `Ctrl+Z` / `Ctrl+Shift+Z` undo and redo,
with slider drags collapsed into a single step.

## Layout

```
crates/
  pe-color/    colour spaces, transfer functions, the two-space pipeline
  pe-core/     document model: stack, parameters, history
  pe-effects/  effect registry — params, shaders, working-space declarations
  pe-render/   wgpu device, 16f textures, the stage cache
  pe-scopes/   waveform / parade / vectorscope / histogram (CPU reference)
  pe-io/       image decode and encode
  pe-ffi/      C ABI surface for Swift
shaders/       .wgsl, shared by every platform
apps/
  windows/     Rust shell — contains no image processing
  macos/       SwiftUI scaffold — built at M6
tests/golden/  reference renders, diffed by CI
docs/          the two decision records worth reading first
```

### The firewall rule

`apps/windows` contains **no image processing whatsoever**. Its entire
vocabulary is: read the stack, mutate a parameter, ask `pe-render` for a
texture, draw it. The day a convenience function that touches pixels appears in
a UI crate is the day the Mac port silently becomes a rewrite.

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
| M6 | macOS | |

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
