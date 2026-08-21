# Photo Editor

DaVinci's colour page, rebuilt for photographs, with a stacked reorderable
inspector instead of a node graph.

**Status: M0 complete.** The colour pipeline, document model and stage cache
exist and are tested. The nine effects arrive at M1.

## Running it

```bash
cargo run -p pe-windows
```

With no argument it shows a built-in test chart. Pass an image to load it:

```bash
cargo run -p pe-windows -- photo.jpg
```

What you get at M0 is the two ends of the pipeline wired together: an image
decoded into a 16-bit float ACEScg working texture and rendered back out to the
display. The effect rows slot in between at M1 without either end changing.

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
| M1 | The engine, proven — nine effects, 60fps on 24MP | |
| M2 | The Colour Page — real UI, scopes, wheels, curves | |
| M3 | Isolation — qualifier, power windows, masks | |
| M4 | Grading workflow — stills gallery, versions, compare | |
| M5 | RAW | |
| M6 | macOS | |

## Known constraint

Pinned to **wgpu 26**. wgpu 30.0.0's DX12 backend depends on `windows` 0.62
while the newest published `gpu-allocator` (0.28) is built against 0.58, so it
does not compile on Windows. Revisit when upstream republishes.
