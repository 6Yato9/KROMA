# The Apple shells

Design for the macOS and iOS/iPadOS front ends, and for the two layers
underneath them that both depend on.

**Scope of this spec:** the shared foundation — a new `pe-session` crate, a
real `pe-ffi`, and the `KromaKit` Swift package — plus a working macOS vertical
slice. The breadth of the Mac colour page and the iOS/iPadOS shell are named
here so the foundation is built to carry them, but they are separate specs.

---

## 1. Why not Electron, and why not Expo

Both were considered and both are refused, for the same reason.

KROMA's premise is that dragging a slider re-renders exactly one cached GPU
stage. The `passes` counter in the toolbar is the product. `pe-render` keeps
every intermediate as `Rgba16Float` on the GPU and never round-trips to the CPU
except at export, and `StageCache` exists so that a nine-row stack costs one
pass when the deepest slider moves.

**Electron** puts a Chromium process between that pipeline and the screen.
There are two ways across it and both give the premise away:

- Recompile the engine to WASM and present through WebGPU. That abandons
  `pe-io`'s ICC handling, the APP2 and `iCCP` writers, 16-bit PNG export, and
  native file access — and it makes every golden test a test of something the
  shipped application no longer runs.
- Blit frames from Rust through a native Node module onto a canvas. That is a
  full-frame CPU copy per slider tick, which is the precise cost `StageCache`
  was written to avoid.

Either way it is roughly 150 MB of runtime around a GPU image viewer.

**Expo / React Native** fails from the other side. The viewer has to be a
`CAMetalLayer` in a native view, so a custom native module is required
regardless — and once it exists, Photos and Files integration, ProRAW, Apple
Pencil and pinch-zoom hit-testing on the viewer are all native code too.
Expo's managed workflow cannot link a Rust static library without prebuild.
What remains for JavaScript is the sliders, in an application whose defining
quality metric is slider-drag latency.

The repository already anticipated the answer. `GpuContext::from_parts` exists
to adopt a device someone else created, `pe-ffi` is a `staticlib`, and
`apps/macos` is XcodeGen plus SwiftUI. **Native Swift and Metal, over an
expanded `pe-ffi`.**

## 2. Decisions

| | |
|---|---|
| **Shells** | Native Swift. SwiftUI for structure, AppKit/UIKit where SwiftUI cannot reach. |
| **Order** | Shared foundation, then macOS, then iOS/iPadOS. |
| **Truth** | Rust owns document, history and GPU state. Swift mirrors it. |
| **Viewer** | Swift hands Rust a `CAMetalLayer`; Rust builds the wgpu surface. |
| **Workflow rules** | Extracted from `apps/windows` into `crates/pe-session`, incrementally, with the Windows app converted onto each piece as it lands. |
| **Distribution** | Notarised direct download. Sandbox-ready file layer, sandbox off. |
| **First milestone** | A vertical slice, not parity. |

## 3. Layout

```
crates/
  pe-color, pe-core, pe-effects, pe-render, pe-scopes, pe-io   unchanged
  pe-session/   NEW. The workflow layer.
  pe-ffi/       Thin C ABI over pe-session. Grows from 175 lines.
apps/
  windows/      Converted onto pe-session. Gets smaller.
  apple/        Renamed from apps/macos; hosts both Apple targets.
    KromaKit/   NEW. Swift package shared by both shells.
    Kroma-macOS/
    Kroma-iOS/
    project.yml
```

`apps/macos` becomes `apps/apple` because it will hold two platforms and a
package shared between them, and a directory named for one of the three is a
directory that will be lied about in every future commit message.

### The firewall rule, restated

`apps/windows` contains no image processing. That rule holds and is extended:
**the shells contain no workflow rules either.** "Never write over an
original", "a collision is refused rather than resolved", "the autosave wins
when a photograph is opened" are behaviour, not interface, and behaviour
implemented twice is behaviour that will differ.

## 4. `pe-session` — the workflow layer

Today `apps/windows` holds about 1,500 lines that are neither UI nor image
processing:

| file | what it owns |
|---|---|
| `library.rs` | the open set, per-photo document and history, folder scan, threaded thumbnails |
| `autosave.rs` | the autosave store, the FNV-1a key, the debounce |
| `settings.rs` | favourites, session restore |
| `main.rs` | export naming, the `_KROMA` suffix, the collision refusal |

A Swift application would rewrite every one of them. `pe-session` is where
they move instead.

### What it owns

- **The open set.** Paths, per-photo `Document` and `History`, the selected
  index, and the 128px thumbnails on their worker thread and channel — moved
  from `library.rs` intact.
- **The render session.** A `GpuContext`, a `StageCache`, an `EffectRenderer`,
  the attached surface, and the view state (zoom and pan) that `Region` is
  derived from.
- **Autosave.** Including the debounce and the FNV-1a key, which stays exactly
  as it is: it is fixed forever by being sixteen lines long, and changing it
  orphans work in progress.
- **Settings.** Favourites and session restore.
- **Export.** Naming, the collision check against every file in the open set,
  format and quality, and batch progress.

### What it does not own

No image processing (that is `pe-render`), no widgets, no window, no event
loop, and **no opinion about where on disk anything lives**. See below.

### The support directory is given, not guessed

`autosave.rs::dir()` currently branches on `cfg!(windows)` and otherwise falls
through to `XDG_CONFIG_HOME`, so on a Mac it would write to `~/.config/Kroma`,
which is not where a Mac application keeps anything. On iOS the answer is a
container path that is not knowable from inside Rust at all.

So `pe-session` does not ask. The host calls
`Session::set_support_dir(path)` once at start-up:

| host | path |
|---|---|
| Windows | `%APPDATA%\Kroma` |
| macOS | `~/Library/Application Support/Kroma` |
| iOS | the app container's Application Support |

This also removes the last `cfg!` from code that is meant to be
platform-independent.

### Sources and paths

`pe-session` works in `std::path::Path`. Making a path valid is the host's job:
on macOS with the sandbox off it already is; under a sandbox and on iOS,
`KromaKit` resolves a security-scoped bookmark and starts access before calling
in. This keeps `pe-io`, `pe-session` and the Windows shell on one code path and
confines the platform difference to Swift.

`Document::Source` stays `Path | Embedded`. A Photos asset has no stable path
and will need a third variant, but that decision belongs to the iOS spec, not
to this one.

### Extraction is incremental

Each piece moves in its own commit, and `apps/windows` is converted onto it in
the same commit. The Windows app is the proof that the extraction was
faithful — it has the tests and it has a person who will notice. Nothing is
copied; things are moved.

## 5. `pe-ffi` — the C ABI

The three existing rules stand: opaque pointers, primitives and UTF-8 C strings
only; every allocation has a matching `pe_*_free`; nothing unwinds across the
boundary. Two more are added.

**4. Hot paths are typed scalars. Cold paths are JSON.** A slider drag must not
allocate a string. Structure — opening a document, describing the registry,
listing the stack — is rare and shape-heavy, so it is JSON, where adding a
field does not mean adding a function.

**5. The engine never calls back into Swift.** Swift drives; Rust answers.
Background work (thumbnails, batch export) is collected by polling
`pe_session_poll_events`, which returns JSON, from the display link. A callback
into Swift from a Rust worker thread is a reentrancy bug waiting for a
deadline.

### Surface

Roughly fifty functions in ten groups. Every fallible call returns `i32`
(`0` = ok, negative = an error code); `pe_session_last_error` yields the
message.

```
lifecycle    pe_session_new / _free / _last_error / _set_support_dir
display      pe_session_attach_layer(layer, w, h, scale) / _resize / _detach
render       pe_session_render / _needs_render / _set_view(zoom, pan_x, pan_y)
             pe_session_last_passes()   the number the toolbar shows
library      pe_session_open_path / _open_folder / _select / _photo_count
             pe_session_photos_json / _thumbnail(index) -> RGBA8
document     pe_session_snapshot_json / _snapshot_version
             pe_session_add_effect / _remove_row / _move_row
             pe_session_set_row_enabled / _set_row_opacity / _set_row_blend
             pe_session_set_geometry_json    crop and straighten
             pe_session_set_color_settings(input, output)
parameters   pe_session_set_float / _set_bool / _set_choice / _set_rgb
             pe_session_set_wheel(row, key, master, r, g, b)
             pe_session_set_param_json / _get_param_json   (Curve, Warp, Pins)
history      pe_session_begin_interaction(label) / _end_interaction
             pe_session_undo / _redo / _can_undo / _can_redo
registry     pe_registry_json()
export       pe_session_export / _export_all / _export_progress
persistence  pe_session_save_sidecar / _load_sidecar / _revert / _tick
settings     pe_session_settings_json / _set_favourite(key, on)
```

`pe_registry_json` is the one that decides how large the Swift application is.
See §6.

`pe_session_snapshot_version` is a monotonic counter bumped by every mutation.
Swift compares it before decoding, so an unchanged frame costs one integer
rather than a JSON parse.

`begin_interaction` / `end_interaction` are how a slider drag collapses into
one undo step — the same bracket `apps/windows` already applies, exposed rather
than reimplemented.

## 6. `KromaKit` — the shared Swift package

One package, both platforms, six modules.

- **`Engine`** — the only file that touches the C ABI, which is the rule
  `Engine.swift` already states. A `final class Session` wraps the opaque
  handle; `deinit` frees it. Everything above works in Swift types.
- **`Snapshot`** — `Codable` structs decoded from `pe_session_snapshot_json`:
  the stack rows with their effect key, `enabled`, `opacity`, `blend` and
  parameters, plus geometry, colour settings, history depth and the current
  photo. Immutable, and derived — never authored. An `@Observable
  SessionStore` holds the current one, calls into `Session` to mutate, and
  refreshes.
- **`Registry`** — decoded once at launch from `pe_registry_json()` into Swift
  `EffectDef` / `ParamDef` / `ParamKind` enums with associated values.
- **`Controls`** — eight views, one per `ParamKind`: `FloatSliderRow`,
  `BoolRow`, `ChoiceRow`, `RgbRow`, `WheelView`, `CurveEditor`, `WarpGrid`,
  `PinsView` — plus `ParameterRow`, the four-column Resolve row (right-aligned
  label, thin track, boxed number, reset arrow) whose column widths come
  straight from `resolve.rs`.
- **`Viewer`** — `MetalViewerView`, an `NSViewRepresentable` /
  `UIViewRepresentable` over a layer-backed view whose `CAMetalLayer` is handed
  to `pe_session_attach_layer`. SwiftUI overlays composite above it in a
  `ZStack` with native hit-testing, which is how crop handles and mask controls
  will work.
- **`FileAccess`** — a protocol that vends URLs. On macOS today it is a
  passthrough; its shape is bookmark-based so that turning the sandbox on is an
  entitlement and one conformance, not a rewrite.

### Why the registry is the whole trick

`apps/windows/src/inspector.rs:653` is a single `match` on `ParamKind` that
renders every control for all thirty effects. Nothing is written per effect.
That is why the Windows shell is 13.6k lines and not 40k.

`KromaKit` copies that exactly. Implement eight control views and the entire
inspector falls out — for every effect that exists and every effect added
later. Adding an effect in Rust makes it appear on Windows, macOS and iPadOS
with no Swift changes at all. `Gate` travels in the same JSON, so controls that
cannot do anything are greyed out on every platform for one reason living in
one place.

## 7. The interaction model

A slider drag, end to end:

1. `onEditingChanged(true)` → `session.beginInteraction("Exposure")`.
2. Each drag frame → `session.setFloat(rowID, "ev", v)`, mark needs-render.
   **No snapshot refresh.** The control holds the in-flight value locally, so
   the cost per frame is one FFI call and one render of one cached stage.
3. `onEditingChanged(false)` → `session.endInteraction()` → refresh snapshot.

One FFI call per frame, one undo step per drag, and SwiftUI is never asked to
diff a document while a finger is down.

Structural edits — add an effect, reorder a row, toggle enabled — refresh the
snapshot immediately. They are rare and they change shape.

## 8. Threading and the render loop

The session is single-threaded and lives on the main actor. Every FFI call is
made from the main thread, and rendering happens in the display-link callback,
which is what `eframe` does on Windows today.

Rendering is **on demand**, not continuous. `CADisplayLink` (both platforms;
macOS 14 has `NSView.displayLink(target:selector:)`) ticks, and the callback
calls `pe_session_tick`, then `pe_session_needs_render`, and renders only if
the answer is yes. A photo editor that redraws 120 times a second while nothing
moves is a laptop with a warm keyboard.

Work that must not block — thumbnail decode, batch export — keeps its worker
threads inside `pe-session`, exactly as `library.rs` runs them now, and results
are collected by `pe_session_poll_events` from the same display-link tick.

## 9. Display colour management

This is where the Mac build is better than the Windows one, nearly for free.

`pe-render` already works in `Rgba16Float` and already converts to the
document's chosen output space. On Windows that is then flattened by the
swapchain. On Apple platforms the layer can be told the truth:
`CAMetalLayer.colorspace` set from the snapshot's output space, a
`bgra10_xr`/`rgba16Float` pixel format, and `wantsExtendedDynamicRangeContent`
on an XDR display.

A Display P3 file from a phone would then render as Display P3 on a P3 display
rather than being flattened — which is what the README already promises about
*reading* files, extended to showing them.

Setting a `CAMetalLayer` property is CoreAnimation, not GPU work, so it stays
in Swift and does not breach the firewall. Treated as a goal to validate during
the slice, with an 8-bit sRGB surface as the fallback if wgpu 27's Metal
surface configuration will not take the format.

## 10. Distribution and file access

Notarised direct download — a universal DMG from GitHub releases. No review
turnaround, no store cut, and it suits an MIT/Apache project. Requires an Apple
Developer account for the Developer ID certificate.

The sandbox stays **off** on macOS for now: browsing a folder of five hundred
photographs and keeping an autosave store are both simpler without it. But all
file access in `KromaKit` goes through `FileAccess`, whose shape is
bookmark-based, because iOS mandates that path regardless. Enabling the sandbox
later is an entitlement plus one conformance.

## 11. Testing

- **Rust.** `pe-session` carries unit tests for the rules it now owns — the
  collision refusal, export naming, autosave round-trip and key stability. The
  golden tests keep covering render and are untouched. `apps/windows` being
  converted onto each extracted piece is itself the regression test for the
  extraction.
- **FFI.** `pe-ffi`'s existing test module extends to the new surface: null
  handles, freeing null, malformed input, a document from the future, and a
  snapshot round-trip. These already run under `cargo test --workspace` on
  `macos-latest` in CI.
- **The cross-language seam.** A Rust test writes `registry.json` and
  `snapshot.json` fixtures into `apps/apple/KromaKit/Tests/Fixtures/`; a Swift
  test decodes them into `Registry` and `Snapshot`. If a field is added in Rust
  and not in Swift, or renamed on one side, a test fails on the other. This is
  the only defence against the two halves drifting, and it is cheap.
- **CI.** A macOS job runs `xcodegen generate` and `xcodebuild test` for
  `KromaKit`, alongside the existing matrix.

## 12. The vertical slice — definition of done

Not parity. The slice proves every boundary once, so that everything after it
is filling in known shapes.

1. `Kroma.app` launches. Universal, `arm64` and `x86_64`.
2. `⌘O` opens a JPEG or PNG. It appears in the Metal viewer. Scroll zooms
   anchored under the cursor; double-click fits.
3. The pinned **Basic** panel is generated from `pe_registry_json` — sliders,
   number boxes, reset arrows, correct labels, ranges and neutrals.
4. Dragging Exposure updates the viewer at interactive rates, and
   `pe_session_last_passes` reads `1` — the stage cache re-running only what
   changed, across the FFI as it does in process on Windows.
5. `⌘Z` / `⇧⌘Z` undo and redo, with a drag collapsed into one step.
6. Autosave writes to `~/Library/Application Support/Kroma/edits` and restores
   the edit when the photograph is reopened.
7. `⌘E` exports `NAME_KROMA.jpg` beside the original, and refuses a collision.
8. `cargo test --workspace` green on macOS; `KromaKit` tests green.

After the slice, the remaining twenty-six effects arrive with no Swift work
beyond the eight control types. The real remaining work on the Mac is the
custom-drawn views: curve editor, colour warper, scopes, crop, filmstrip.

## 13. Deferred, deliberately

- **The iOS/iPadOS shell.** Its own spec. It reuses everything below the views
  and needs a touch-first layout, `PHPickerViewController` and Files
  integration, a Photos-asset `Source` variant, and Apple Pencil.
- **RAW.** M5 on the roadmap; `pe-io` gains it once, both platforms get it.
- **The Mac App Store.** The file layer is built so this stays possible.

## 14. Risks

| risk | response |
|---|---|
| wgpu 27's exact API for building a surface from a `CAMetalLayer` | Validated in the first task, before anything is built on it. If it is unavailable, `SurfaceTargetUnsafe` with a raw window handle is the fallback. |
| No Rust toolchain on the development Mac | `rustup` plus the two Apple targets, as a prerequisite step. |
| Extraction into `pe-session` destabilises a working Windows app | One piece per commit, Windows converted in the same commit, `cargo test --workspace` between each. |
| `Rgba16Float`/EDR surface not configurable through wgpu 27 | Fall back to 8-bit sRGB presentation, which is what Windows does today. Nothing else depends on it. |
| Snapshot JSON becomes a per-frame cost | It is not on the drag path by design, and `snapshot_version` gates the decode. If structural edits ever become hot, individual typed getters are added for those fields. |
