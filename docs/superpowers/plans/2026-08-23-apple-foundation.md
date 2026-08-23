# Apple Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust half of the Apple port — a `pe-session` crate that owns the workflow rules, a `pe-ffi` that exposes them to Swift, and a proven `CAMetalLayer` render path — so that the macOS shell is afterwards pure UI work on a verified base.

**Architecture:** Workflow rules (autosave, export naming, the collision refusal, the open document and its history) move out of `apps/windows` into a new `crates/pe-session`, one piece per commit, with the Windows app converted onto each piece as it lands so its existing tests keep proving the move was faithful. `pe-ffi` becomes a thin C ABI over `pe-session`. Swift hands a `CAMetalLayer` down and Rust builds the wgpu surface from it, so every line of GPU code stays on the Rust side.

**Tech Stack:** Rust 2024 edition (toolchain `stable`, rust-version 1.95), wgpu 27.0.1, serde/serde_json, pollster 0.4, cbindgen, XcodeGen, Swift 6 / SwiftUI, Xcode 26.

**Spec:** `docs/superpowers/specs/2026-08-23-apple-shells-design.md`

**Scope:** This plan covers §4 (`pe-session`), §5 (`pe-ffi`), and the `CAMetalLayer` risk from §14. The `KromaKit` package (§6) and the macOS shell (§12) are a second plan, written once this one is green — their Swift code depends on the generated `pe_ffi.h` and on the outcome of Task 3, and writing it before those exist would be writing against types nobody has compiled.

---

## File Structure

**Created:**

| path | responsibility |
|---|---|
| `crates/pe-session/Cargo.toml` | crate manifest |
| `crates/pe-session/src/lib.rs` | public surface, `SessionError` |
| `crates/pe-session/src/support.rs` | where the application keeps its own files — given by the host |
| `crates/pe-session/src/surface.rs` | the attached `CAMetalLayer` and its wgpu surface |
| `crates/pe-session/src/autosave.rs` | the autosave store, key and debounce |
| `crates/pe-session/src/export.rs` | export naming, the collision refusal, writing a file |
| `crates/pe-session/src/session.rs` | the session: open photo, document, history, render |
| `crates/pe-session/src/describe.rs` | registry and snapshot as JSON, for Swift |
| `apps/apple/` | renamed from `apps/macos`; holds both Apple targets |
| `apps/apple/Spike/` | the minimal AppKit harness that proves the layer path |

**Modified:**

| path | change |
|---|---|
| `Cargo.toml` | add `crates/pe-session` to workspace members |
| `crates/pe-ffi/Cargo.toml` | depend on `pe-session` |
| `crates/pe-ffi/src/lib.rs` | grow from 175 lines to the surface in spec §5 |
| `apps/windows/src/autosave.rs` | deleted; call sites move to `pe_session::autosave` |
| `apps/windows/src/main.rs` | export helpers deleted; call sites move to `pe_session::export` |
| `apps/windows/Cargo.toml` | depend on `pe-session` |
| `apps/apple/build-engine.sh` | paths updated for the rename |
| `apps/apple/project.yml` | paths updated; spike target added |
| `.github/workflows/ci.yml` | macOS job builds the spike |

---

## Task 1: Toolchain, and a green baseline

Nothing in this plan can be verified without a Rust toolchain, and this machine has none. This task also proves the claim the CI matrix makes — that the engine crates build and pass on macOS — on *this* Mac rather than on GitHub's.

**Files:**
- Modify: `rust-toolchain.toml`

- [ ] **Step 1: Install rustup and the stable toolchain**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile default
```

Then make it available in this shell:

```bash
source "$HOME/.cargo/env" && rustc --version && cargo --version
```

Expected: a version line for each, `rustc 1.9x.x` or newer. If `rustc` reports older than 1.95, run `rustup update stable`.

- [ ] **Step 2: Add the Apple targets**

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Expected: both report installed (or "up to date").

- [ ] **Step 3: Record the targets in the toolchain file**

`rust-toolchain.toml` already carries a comment saying these land as the Mac port approaches. Replace the commented line with the real one:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"]
```

- [ ] **Step 4: Run the whole test suite**

```bash
cargo test --workspace
```

Expected: PASS. Golden tests included — they need a GPU adapter, which a Mac has.

If golden tests fail, **stop and look at the diff images** in `tests/golden/out/` before doing anything else. A Metal backend producing different output from the CI reference is a real finding about the engine and is worth more than this plan. Do not regenerate the goldens.

- [ ] **Step 5: Confirm formatting and lints are clean**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
```

Expected: no output from either. This is the bar CI holds every later commit to.

- [ ] **Step 6: Commit**

```bash
git add rust-toolchain.toml
git commit -m "The toolchain file names the two Apple targets it was always going to need"
```

---

## Task 2: The `pe-session` crate, and a support directory that is given rather than guessed

`apps/windows/src/autosave.rs:38` decides where to write by branching on `cfg!(windows)` and otherwise falling through to `XDG_CONFIG_HOME`. On a Mac that lands in `~/.config/Kroma`, which is not where a Mac application keeps anything, and on iOS the answer is a container path that cannot be known from inside Rust at all.

So the new crate does not ask. The host says.

**Files:**
- Create: `crates/pe-session/Cargo.toml`
- Create: `crates/pe-session/src/lib.rs`
- Create: `crates/pe-session/src/support.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add `"crates/pe-session",` to `members`, after `"crates/pe-io",`:

```toml
members = [
    "crates/pe-color",
    "crates/pe-core",
    "crates/pe-effects",
    "crates/pe-render",
    "crates/pe-scopes",
    "crates/pe-io",
    "crates/pe-session",
    "crates/pe-ffi",
    "apps/windows",
    "tests/golden",
]
```

- [ ] **Step 2: Write the manifest**

Create `crates/pe-session/Cargo.toml`:

```toml
[package]
name = "pe-session"
description = "The workflow layer: the open photograph, its edit, and the rules about writing files"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
pe-color = { version = "0.0.1", path = "../pe-color" }
pe-core = { version = "0.0.1", path = "../pe-core" }
pe-effects = { version = "0.0.1", path = "../pe-effects" }
pe-io = { version = "0.0.1", path = "../pe-io" }
pe-render = { version = "0.0.1", path = "../pe-render" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2.0.20"
wgpu = "27"
pollster = "0.4"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write the failing test**

Create `crates/pe-session/src/support.rs` containing **only** the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_support_directory_nobody_set_yields_no_paths() {
        // Nothing is written until the host says where. A default that guesses
        // is how an application ends up sprinkling files somewhere nobody
        // asked for, on a platform nobody tested.
        let s = Support::default();
        assert!(s.root().is_none());
        assert!(s.edits_dir().is_none());
        assert!(s.settings_path().is_none());
    }

    #[test]
    fn the_paths_hang_off_the_root_the_host_gave() {
        let s = Support::at("/Users/someone/Library/Application Support/Kroma");
        assert_eq!(
            s.edits_dir().unwrap(),
            std::path::Path::new("/Users/someone/Library/Application Support/Kroma/edits")
        );
        assert_eq!(
            s.settings_path().unwrap(),
            std::path::Path::new("/Users/someone/Library/Application Support/Kroma/settings.json")
        );
    }
}
```

And create `crates/pe-session/src/lib.rs`:

```rust
//! The workflow layer.
//!
//! Between the engine and the shells. It owns the things that are neither
//! image processing nor interface: which photograph is open, what its edit is,
//! where work in progress is kept, and the rules about what may be written
//! where.
//!
//! Those rules lived in `apps/windows` until the Mac port needed them too, and
//! a rule implemented twice is a rule that will differ. "Never write over an
//! original" is not a Windows rule.

pub mod support;

pub use support::Support;
```

- [ ] **Step 4: Run it and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile, `cannot find type 'Support' in this scope`.

- [ ] **Step 5: Write the implementation**

Put this **above** the test module in `crates/pe-session/src/support.rs`:

```rust
//! Where the application keeps what belongs to it rather than to a photograph.
//!
//! Given by the host, never guessed. Rust cannot know that a Mac wants
//! `~/Library/Application Support`, that Windows wants `%APPDATA%`, and that an
//! iPad wants a container path which does not exist until the process starts.
//! A `cfg!` that tries is a `cfg!` sitting in code whose whole purpose is to be
//! platform-independent — and it was already wrong, silently, on the Mac.
//!
//! Unset means *write nothing*. A host that has not said where has not agreed
//! to anything being written, and a default that guesses would be an
//! application putting files somewhere nobody chose.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Support {
    root: Option<PathBuf>,
}

impl Support {
    /// Keep our files under `root`. The host supplies this once, at start-up.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Where autosaved work in progress is kept, one file per photograph.
    pub fn edits_dir(&self) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join("edits"))
    }

    /// Where the things belonging to the person rather than to a picture live.
    pub fn settings_path(&self) -> Option<PathBuf> {
        Some(self.root.as_ref()?.join("settings.json"))
    }
}
```

- [ ] **Step 6: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 2 tests.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/pe-session
git commit -m "A crate for the rules, and a support directory the host names rather than Rust guessing"
```

---

## Task 3: The Metal layer, proven

The largest unknown in the whole port is whether wgpu 27 will build a surface from a `CAMetalLayer` handed over by Swift. Everything else in this plan is ordinary work; this is the part that could be wrong. It goes first, and it gets a real Swift harness rather than a Rust test, because a Rust test cannot make a `CAMetalLayer`.

**Files:**
- Create: `crates/pe-session/src/surface.rs`
- Modify: `crates/pe-session/src/lib.rs`
- Rename: `apps/macos` → `apps/apple`
- Create: `apps/apple/Spike/main.swift`
- Create: `apps/apple/Spike/Spike-Bridging-Header.h`
- Modify: `apps/apple/project.yml`, `apps/apple/build-engine.sh`, `apps/apple/README.md`

- [ ] **Step 1: Rename the directory**

```bash
git mv apps/macos apps/apple
```

It will hold macOS, iOS and a package shared between them. A directory named for one of the three is a directory that gets lied about in every later commit message.

- [ ] **Step 2: Write the surface module**

Create `crates/pe-session/src/surface.rs`:

```rust
//! The window's layer, as a wgpu surface.
//!
//! On Apple platforms the host owns the view hierarchy and we are given a
//! `CAMetalLayer` to draw into. That is the whole of the platform-specific
//! surface story: no window creation, no event loop, no swapchain management
//! beyond configuring one.
//!
//! The instance is held by the caller rather than created here, because a
//! `Surface` belongs to the `Instance` that made it and an adapter obtained
//! from a different instance cannot present to it — the failure is a panic
//! deep inside wgpu, a long way from the cause. `GpuContext::create_instance`
//! exists for exactly this split.

use std::ffi::c_void;

use wgpu::TextureFormat;

/// A layer we have been given, and the surface configured onto it.
pub struct Attached {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

impl Attached {
    /// Build a surface on a `CAMetalLayer`.
    ///
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives this `Attached`.
    /// The Swift side guarantees this by keeping the layer on a view it owns
    /// and detaching before the view goes away.
    pub unsafe fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        layer: *mut c_void,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| e.to_string())?;

        let caps = surface.get_capabilities(adapter);
        // Ask for an sRGB-encoding target so the transfer function is applied
        // on write by the hardware, which is what the Windows display texture
        // does through `SOURCE_FORMAT`. Falling back to whatever the surface
        // does offer keeps this from being a hard failure on a device that
        // surprises us.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == TextureFormat::Bgra8UnormSrgb)
            .unwrap_or_else(|| caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(device, &config);
        Ok(Self { surface, config })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(device, &self.config);
    }

    /// Fill the surface with one colour and present it.
    ///
    /// The proof that the layer path works, and afterwards the thing that
    /// draws the background behind a photograph that has not loaded.
    pub fn present_clear(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        colour: [f64; 4],
    ) -> Result<(), String> {
        let frame = self
            .surface
            .get_current_texture()
            .map_err(|e| e.to_string())?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clear") });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: colour[0],
                        g: colour[1],
                        b: colour[2],
                        a: colour[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}
```

Add to `crates/pe-session/src/lib.rs`:

```rust
pub mod surface;

pub use surface::Attached;
```

- [ ] **Step 3: Check it compiles, and fix the API if it does not**

```bash
cargo check -p pe-session
```

Expected: PASS.

**If `SurfaceTargetUnsafe::CoreAnimationLayer` does not exist**, print the variants and pick the Apple one:

```bash
cargo doc -p wgpu --no-deps --open
```

The documented fallback is `SurfaceTargetUnsafe::RawHandle { raw_display_handle, raw_window_handle }` with a `raw_window_handle::AppKitWindowHandle`. `raw-window-handle` 0.6.2 is already in the lock file. **If neither works, stop and report** — the spec's viewer decision depends on this and would need revisiting.

**If `depth_slice` is not a field** of `RenderPassColorAttachment` in wgpu 27, remove that line; it was added around this version and its presence is the only uncertain part of the render-pass literal.

- [ ] **Step 4: Write the Swift harness**

Create `apps/apple/Spike/Spike-Bridging-Header.h`:

```c
#import "pe_ffi.h"
```

Create `apps/apple/Spike/main.swift`:

```swift
// The smallest thing that can tell us whether the layer path works.
//
// A window, a layer-backed view, and one call into Rust asking it to fill the
// layer. If this window comes up orange, wgpu built a surface on a CAMetalLayer
// that Swift created, and the viewer decision in the spec holds.

import AppKit
import QuartzCore

final class SpikeView: NSView {
    private var attached = false

    override func makeBackingLayer() -> CALayer { CAMetalLayer() }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .duringViewResize
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard !attached, let metal = layer as? CAMetalLayer else { return }
        metal.contentsScale = window?.backingScaleFactor ?? 2.0
        metal.drawableSize = CGSize(
            width: bounds.width * metal.contentsScale,
            height: bounds.height * metal.contentsScale
        )
        let ptr = Unmanaged.passUnretained(metal).toOpaque()
        let rc = pe_spike_attach_and_clear(
            ptr,
            UInt32(metal.drawableSize.width),
            UInt32(metal.drawableSize.height)
        )
        attached = true
        if rc != 0 {
            print("attach failed, rc=\(rc)")
            NSApp.terminate(nil)
        } else {
            print("attached and cleared")
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let window = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: 640, height: 400),
    styleMask: [.titled, .closable],
    backing: .buffered,
    defer: false
)
window.title = "Kroma spike"
window.contentView = SpikeView(frame: window.contentRect(forFrameRect: window.frame))
window.center()
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
app.run()
```

- [ ] **Step 5: Add the one FFI function the spike needs**

Append to `crates/pe-ffi/src/lib.rs`, and add `pe-session = { version = "0.0.1", path = "../pe-session" }` and `wgpu = "27"` and `pollster = "0.4"` to `crates/pe-ffi/Cargo.toml` under `[dependencies]`:

```rust
/// Prove a `CAMetalLayer` can be drawn into. Fills it with orange.
///
/// Temporary — replaced by `pe_session_attach_layer` in Task 10. It exists so
/// the riskiest assumption in the port is tested before anything is built on
/// it.
///
/// # Safety
/// `layer` must be a live `CAMetalLayer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_spike_attach_and_clear(
    layer: *mut std::ffi::c_void,
    width: u32,
    height: u32,
) -> i32 {
    guard(-99, || {
        if layer.is_null() {
            return -1;
        }
        let instance = pe_render::GpuContext::create_instance();
        // Adapter must come from the same instance the surface will belong to,
        // and must be told about the surface so a machine with more than one
        // GPU picks one that can present to it.
        let probe = match unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        } {
            Ok(s) => s,
            Err(_) => return -2,
        };
        let Ok(gpu) = pollster::block_on(pe_render::GpuContext::from_instance(
            &instance,
            Some(&probe),
        )) else {
            return -3;
        };
        drop(probe);
        let attached = match unsafe {
            pe_session::Attached::new(
                &instance,
                &gpu.adapter,
                &gpu.device,
                layer,
                width,
                height,
            )
        } {
            Ok(a) => a,
            Err(_) => return -4,
        };
        match attached.present_clear(&gpu.device, &gpu.queue, [0.85, 0.45, 0.10, 1.0]) {
            Ok(()) => 0,
            Err(_) => -5,
        }
    })
}
```

Add `use pe_render as _;` is not needed — add `pe-render = { version = "0.0.1", path = "../pe-render" }` to `crates/pe-ffi/Cargo.toml` as well.

- [ ] **Step 6: Point the build script and the project at the new paths**

In `apps/apple/build-engine.sh`, change the header output line from `apps/macos/PhotoEditor/pe_ffi.h` to:

```bash
  cbindgen --config cbindgen.toml --crate pe-ffi \
           --output apps/apple/PhotoEditor/pe_ffi.h
```

and add a second copy for the spike, immediately after it:

```bash
  cp apps/apple/PhotoEditor/pe_ffi.h apps/apple/Spike/pe_ffi.h
```

In `apps/apple/project.yml`, add the spike target after the existing `PhotoEditor` target, keeping the same indentation:

```yaml
  Spike:
    type: application
    platform: macOS
    sources:
      - path: Spike
    settings:
      base:
        SWIFT_OBJC_BRIDGING_HEADER: Spike/Spike-Bridging-Header.h
        LIBRARY_SEARCH_PATHS: $(SRCROOT)/../../target/universal/$(CONFIGURATION:lower)
        OTHER_LDFLAGS: -lpe_ffi
        HEADER_SEARCH_PATHS: $(SRCROOT)/Spike
    preBuildScripts:
      - name: Build Rust engine
        script: "\"$SRCROOT/build-engine.sh\""
        basedOnDependencyAnalysis: false
```

Add `pe_ffi.h` to `.gitignore` so the generated header is never committed:

```bash
printf '\n# Generated by apps/apple/build-engine.sh\napps/apple/*/pe_ffi.h\n' >> .gitignore
```

- [ ] **Step 7: Install cbindgen and build the spike**

```bash
cargo install cbindgen --locked
```

```bash
cd apps/apple && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme Spike -configuration Debug build
```

Expected: `BUILD SUCCEEDED`.

- [ ] **Step 8: Run it and look at the window**

```bash
cd apps/apple && open "$(xcodebuild -project PhotoEditor.xcodeproj -scheme Spike -configuration Debug -showBuildSettings | awk -F' = ' '/ BUILT_PRODUCTS_DIR/ {print $2}')/Spike.app"
```

Expected: **a 640×400 window filled with orange**, and `attached and cleared` on the console.

A black window means the clear did not reach the layer. A window that never appears means a panic — check Console.app. Either way the return code printed by the harness says which of the five stages failed, and **that number is the finding**: report it rather than working around it.

- [ ] **Step 9: Commit**

```bash
git add -A apps/apple crates/pe-session crates/pe-ffi .gitignore Cargo.lock
git commit -m "A CAMetalLayer made in Swift, filled by wgpu in Rust"
```

---

## Task 4: Autosave moves, and stops guessing

The store moves wholesale. The only change to its behaviour is that the
directory arrives as an argument instead of being worked out from `cfg!`.

**Files:**
- Create: `crates/pe-session/src/autosave.rs`
- Delete: `apps/windows/src/autosave.rs`
- Modify: `crates/pe-session/src/lib.rs`, `apps/windows/src/main.rs`, `apps/windows/Cargo.toml`

- [ ] **Step 1: Write the failing tests**

Create `crates/pe-session/src/autosave.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn doc_with_note(note: &str) -> Document {
        let mut d = Document::from_path("photo.jpg");
        d.metadata.note = Some(note.to_string());
        d
    }

    #[test]
    fn an_edit_comes_back_from_the_store_it_was_put_in() {
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"not really a jpeg").unwrap();

        store(&support, &photo, &doc_with_note("in progress"));
        let back = load(&support, &photo).expect("stored, so it loads");
        assert_eq!(back.metadata.note.as_deref(), Some("in progress"));
    }

    #[test]
    fn nothing_is_written_when_the_host_never_said_where() {
        // The whole point of Support being an Option. A store with nowhere to
        // go writes nothing rather than choosing somewhere.
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();

        store(&Support::default(), &photo, &doc_with_note("lost"));
        assert!(load(&Support::default(), &photo).is_none());
        // And nothing appeared beside the photograph either.
        let beside: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert_eq!(beside.len(), 1, "something was written next to the photo");
    }

    #[test]
    fn one_photographs_edit_is_never_handed_to_another() {
        // The recorded source is the collision check. Two paths hashing to one
        // name is unlikely; being read as each other's edits is unacceptable.
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let a = tmp.path().join("a.jpg");
        let b = tmp.path().join("b.jpg");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"y").unwrap();

        store(&support, &a, &doc_with_note("belongs to a"));
        assert!(load(&support, &b).is_none());
    }

    #[test]
    fn forgetting_leaves_nothing_to_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let support = Support::at(tmp.path());
        let photo = tmp.path().join("a.jpg");
        std::fs::write(&photo, b"x").unwrap();

        store(&support, &photo, &doc_with_note("temporary"));
        forget(&support, &photo);
        assert!(load(&support, &photo).is_none());
    }

    #[test]
    fn nothing_is_written_while_the_value_is_still_moving() {
        let mut w = Watcher::new();
        let start = Instant::now();
        for i in 1..40u64 {
            let at = start + Duration::from_millis(i * 16);
            assert!(!w.tick(i, at), "wrote during a drag, at frame {i}");
        }
    }

    #[test]
    fn a_pause_after_a_change_writes_once() {
        let mut w = Watcher::new();
        let start = Instant::now();
        assert!(!w.tick(1, start));
        assert!(!w.tick(1, start + IDLE / 2));
        assert!(w.tick(1, start + IDLE), "did not write after the pause");
        assert!(
            !w.tick(1, start + IDLE * 3),
            "wrote a second time with nothing new"
        );
    }

    #[test]
    fn leaving_a_photograph_knows_there_is_work_outstanding() {
        let mut w = Watcher::new();
        assert!(!w.pending());
        w.tick(1, Instant::now());
        assert!(w.pending());
        w.reset(1);
        assert!(!w.pending());
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — `store`, `load`, `forget`, `Watcher`, `IDLE`, `Document` unresolved.

- [ ] **Step 3: Move the implementation across**

Copy the whole of the old `apps/windows/src/autosave.rs` above the new test module in `crates/pe-session/src/autosave.rs`, then make exactly these changes:

1. Delete the `dir()` function entirely.
2. Change `path_for` to take the support directory:

```rust
fn path_for(support: &Support, photo: &Path) -> Option<PathBuf> {
    Some(support.edits_dir()?.join(key(photo)))
}
```

3. Give `load`, `store` and `forget` a `support` parameter and pass it through:

```rust
pub fn load(support: &Support, photo: &Path) -> Option<Document> {
    let text = std::fs::read_to_string(path_for(support, photo)?).ok()?;
    ...
}

pub fn store(support: &Support, photo: &Path, document: &Document) {
    let Some(path) = path_for(support, photo) else {
        return;
    };
    ...
}

pub fn forget(support: &Support, photo: &Path) {
    if let Some(path) = path_for(support, photo) {
        let _ = std::fs::remove_file(path);
    }
}
```

4. Fix the imports at the top:

```rust
use std::path::{Path, PathBuf};

use pe_core::Document;
use serde::{Deserialize, Serialize};

use crate::Support;
```

5. Delete the old test module that came with the file — the new one above replaces it and covers strictly more.
6. Update the module doc comment: the paragraph naming `%APPDATA%\Kroma` becomes

```rust
//! It is kept **with the application**, in the support directory the host
//! names — `%APPDATA%\Kroma` on Windows, `~/Library/Application Support/Kroma`
//! on a Mac, the app container on iOS — and not beside your photographs. A
//! photo directory that has never been written to stays that way.
```

**Do not touch `key()`.** Its FNV-1a is fixed forever by being sixteen lines long, and changing it orphans everybody's work in progress.

Add to `crates/pe-session/src/lib.rs`:

```rust
pub mod autosave;
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 9 tests.

- [ ] **Step 5: Convert the Windows app onto it**

Add to `apps/windows/Cargo.toml` under `[dependencies]`:

```toml
pe-session = { version = "0.0.1", path = "../../crates/pe-session" }
```

Delete the file and its module declaration:

```bash
git rm apps/windows/src/autosave.rs
```

In `apps/windows/src/main.rs`, replace the `mod autosave;` line with nothing, and add near the other `use` lines:

```rust
use pe_session::{Support, autosave};
```

Add a field to `struct App`, beside `settings`:

```rust
    /// Where this platform keeps what belongs to the application. The one
    /// place a `cfg!` about directories is correct: the shell knows what
    /// platform it is.
    support: Support,
```

Add the function that fills it, next to `ids_for` near the bottom of `main.rs`:

```rust
/// Where a Windows or Linux build keeps its own files.
///
/// The Mac and iOS shells answer this differently and pass their own answer
/// down, which is why `pe-session` takes it as an argument rather than working
/// it out.
fn platform_support() -> Support {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(v) => Some(PathBuf::from(v)),
            None => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")),
        }
    };
    match base {
        Some(b) => Support::at(b.join("Kroma")),
        None => Support::default(),
    }
}
```

Set it in `App::new` alongside the other field initialisers: `support: platform_support(),`.

Then fix every call site. There are four; find them with:

```bash
LC_ALL=C grep -an "autosave::" apps/windows/src/main.rs
```

Each becomes the same call with `&self.support` as the first argument — for example `autosave::load(path)` becomes `autosave::load(&self.support, path)`. Where the borrow checker objects to `&self.support` beside a `&mut self` field, bind it first: `let support = self.support.clone();`.

- [ ] **Step 6: Run everything**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS, and no clippy output.

- [ ] **Step 7: Commit**

```bash
git add -A crates/pe-session apps/windows Cargo.lock
git commit -m "The autosave store moves to pe-session, and is told where to write"
```

---

## Task 5: The rules about writing files move

`export_name`, `unclaimed_export_path`, `same_file` and `would_overwrite_a_source` are the promises the README makes about never touching an original. They move with their tests, which are the most valuable tests in the repository.

**Files:**
- Create: `crates/pe-session/src/export.rs`
- Modify: `crates/pe-session/src/lib.rs`, `apps/windows/src/main.rs`, `apps/windows/src/settings.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/pe-session/src/export.rs` with only this test module. These are the existing tests from `apps/windows/src/main.rs`, moved and made independent of the `App` struct:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn batch_path(dir: &str, source: &str, format: Format) -> PathBuf {
        let mut taken = HashSet::new();
        unclaimed_export_path(Path::new(dir), Path::new(source), format, &mut taken)
    }

    #[test]
    fn the_export_carries_the_kroma_suffix() {
        assert_eq!(
            export_name(Path::new("/photos/DJI_0001.JPG"), Format::Jpeg),
            "DJI_0001_KROMA.jpg"
        );
    }

    #[test]
    fn an_export_is_never_named_after_its_source() {
        let source = Path::new("/photos/sunset.jpg");
        let out = source.with_file_name(export_name(source, Format::Jpeg));
        assert_ne!(out, source.to_path_buf());
        assert!(!same_file(source, &out));
    }

    #[test]
    fn a_png_source_exported_as_png_is_still_safe() {
        // The case the naming scheme alone would not survive: same extension,
        // same folder. The suffix is what keeps them apart.
        let source = Path::new("/photos/chart.png");
        let out = source.with_file_name(export_name(source, Format::Png));
        assert!(!same_file(source, &out));
    }

    #[test]
    fn two_names_differing_only_in_case_are_the_same_file() {
        // Windows ignores case, so a comparison that did not is exactly the
        // comparison that would let a batch export eat a folder of originals.
        assert!(same_file(Path::new("/p/A_KROMA.jpg"), Path::new("/p/a_kroma.JPG")));
    }

    #[test]
    fn two_photographs_with_one_name_do_not_share_an_export() {
        let mut taken = HashSet::new();
        let out = Path::new("/out");
        let first = unclaimed_export_path(out, Path::new("/a/sunset.jpg"), Format::Jpeg, &mut taken);
        let second = unclaimed_export_path(out, Path::new("/b/sunset.jpg"), Format::Jpeg, &mut taken);
        assert_ne!(first, second);
        assert_eq!(second.file_name().unwrap(), "sunset_KROMA_2.jpg");
    }

    #[test]
    fn export_names_collide_regardless_of_case() {
        let mut taken = HashSet::new();
        let out = Path::new("/out");
        unclaimed_export_path(out, Path::new("/a/Sunset.jpg"), Format::Jpeg, &mut taken);
        let second = unclaimed_export_path(out, Path::new("/b/sunset.jpg"), Format::Jpeg, &mut taken);
        assert_eq!(second.file_name().unwrap(), "sunset_KROMA_2.jpg");
    }

    #[test]
    fn exporting_an_export_does_not_land_on_it() {
        assert_eq!(
            batch_path("/out", "/out/sunset_KROMA.png", Format::Png)
                .file_name()
                .unwrap(),
            "sunset_KROMA_KROMA.png"
        );
    }

    #[test]
    fn a_write_onto_any_open_photograph_is_refused() {
        // Checked against every photograph in the set, not only the one on
        // screen: a batch writes into one folder and the name it builds for
        // photo A can collide with photo B sitting right beside it.
        let open = [
            PathBuf::from("/photos/a.jpg"),
            PathBuf::from("/photos/b.jpg"),
        ];
        assert!(would_overwrite_a_source(&open, Path::new("/photos/B.JPG")));
        assert!(!would_overwrite_a_source(&open, Path::new("/photos/c.jpg")));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — `export_name`, `unclaimed_export_path`, `same_file`, `would_overwrite_a_source`, `Format` unresolved.

- [ ] **Step 3: Write the implementation**

Put this above the test module in `crates/pe-session/src/export.rs`. `Format` moves here from `apps/windows/src/settings.rs` because the naming depends on it and a rule that lives apart from the type it reads is a rule that goes stale:

```rust
//! What may be written, and where.
//!
//! The application does not modify the photograph you opened. It cannot: every
//! write is checked against every file in the open set first, and a collision
//! is refused rather than resolved.
//!
//! The naming and the check are two separate defences on purpose. A scheme
//! that happens to differ is not a guarantee — and it would not hold anyway
//! once you can export a PNG of a PNG, which the File page allows in one click.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What an export is written as.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Format {
    /// Small, universal, and eight bits with a lossy step on top. The right
    /// answer for a photograph that is finished and going somewhere.
    #[default]
    Jpeg,
    /// Eight bits, no lossy step. For anything that will be looked at closely
    /// or composited onto.
    Png,
    /// Sixteen bits. Where the wide working space stops being theoretical: a
    /// gradient pushed about by a dozen rows holds more distinct values than
    /// eight bits can name, and this is the only way out that keeps them.
    Png16,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png | Format::Png16 => "png",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG 8",
            Format::Png16 => "PNG 16",
        }
    }

    /// Whether the export path has to read the frame back at full depth.
    pub fn is_sixteen_bit(self) -> bool {
        self == Format::Png16
    }

    /// Parse the name the FFI uses. Unknown names are JPEG, because an export
    /// that happens is better than one refused over a spelling.
    pub fn from_name(name: &str) -> Format {
        match name.to_ascii_lowercase().as_str() {
            "png" => Format::Png,
            "png16" => Format::Png16,
            _ => Format::Jpeg,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Png16 => "png16",
        }
    }
}

/// The export settings, kept together so they can be handed about as one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Export {
    pub format: Format,
    /// JPEG only, 1-100.
    ///
    /// 95 rather than 100: the last few points of a JPEG quality scale buy
    /// almost nothing you can see and cost a great deal of file, and 100 is
    /// still lossy — a person who wants no loss wants PNG, not a bigger JPEG.
    pub quality: u8,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            format: Format::default(),
            quality: 95,
        }
    }
}

pub fn export_name(source: &Path, format: Format) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());
    format!("{stem}_KROMA.{}", format.extension())
}

/// An export name for this photograph that nothing in this run has used yet.
///
/// A batch writes every photograph into one directory, and the set it is
/// writing can have come from several. Two files called `sunset.jpg` in
/// different folders both want to be `sunset_KROMA.jpg`, and without this the
/// second lands on the first: one file on disc, two successes reported, and
/// nothing anywhere saying which one you kept.
///
/// Numbered rather than refused. Losing an original is unrecoverable and worth
/// being rude about; two of your own exports wanting one name is an ordinary
/// thing with an obvious right answer.
pub fn unclaimed_export_path(
    dir: &Path,
    source: &Path,
    format: Format,
    taken: &mut HashSet<String>,
) -> PathBuf {
    let first = export_name(source, format);
    if taken.insert(first.to_lowercase()) {
        return dir.join(first);
    }
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());
    // Two is where a human starts counting a second one of something.
    for n in 2u32.. {
        let name = format!("{stem}_KROMA_{n}.{}", format.extension());
        if taken.insert(name.to_lowercase()) {
            return dir.join(name);
        }
    }
    unreachable!("u32 ran out of numbers")
}

/// Whether two paths name the same file, as far as can be told without
/// creating one.
///
/// `canonicalize` is the right answer and it only works on files that already
/// exist — which an output path usually does not. So the *directories* are
/// canonicalised, which do exist, and the file names compared without regard
/// to case. Windows treats `photo.JPG` and `photo.jpg` as one file, and a
/// comparison that did not is exactly the comparison that would let a batch
/// export eat a folder of originals.
pub fn same_file(a: &Path, b: &Path) -> bool {
    let dir = |p: &Path| {
        let d = p.parent().unwrap_or(Path::new("."));
        std::fs::canonicalize(d).unwrap_or_else(|_| d.to_path_buf())
    };
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
    match (name(a), name(b)) {
        (Some(x), Some(y)) => x == y && dir(a) == dir(b),
        _ => false,
    }
}

/// Whether writing here would land on a photograph we were given.
///
/// A hard refusal rather than a warning. The application is allowed to be
/// annoying about this exactly once — losing somebody's original is not a thing
/// to recover from, and there is no undo that reaches outside the process.
pub fn would_overwrite_a_source(open: &[PathBuf], out: &Path) -> bool {
    open.iter().any(|p| same_file(p, out))
}
```

Add to `crates/pe-session/src/lib.rs`:

```rust
pub mod export;
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 17 tests.

- [ ] **Step 5: Convert the Windows app onto it**

In `apps/windows/src/settings.rs`, delete the `Format` enum, its `impl`, the `Export` struct and its `Default` impl, and re-export the moved ones so the fifty-odd `settings::Format::` call sites keep compiling:

```rust
pub use pe_session::export::{Export, Format};
```

In `apps/windows/src/main.rs`, delete `export_name`, `unclaimed_export_path` and `same_file`, and the seven tests that moved (`the_export_carries_the_kroma_suffix`, `an_export_is_never_named_after_its_source`, `a_png_source_exported_as_png_is_still_safe`, `two_names_differing_only_in_case_are_the_same_file`, `two_photographs_with_one_name_do_not_share_an_export`, `export_names_collide_regardless_of_case`, `exporting_an_export_does_not_land_on_it`, and the `batch_path` helper). Then add to the imports:

```rust
use pe_session::export::{export_name, same_file, unclaimed_export_path};
```

Rewrite `App::would_overwrite_a_source` to call the shared one:

```rust
    fn would_overwrite_a_source(&self, out: &Path) -> bool {
        let open: Vec<PathBuf> = self
            .path
            .iter()
            .cloned()
            .chain(self.library.paths().iter().map(|p| p.to_path_buf()))
            .collect();
        pe_session::export::would_overwrite_a_source(&open, out)
    }
```

If `Library::paths()` already returns `Vec<PathBuf>` rather than a slice of references, drop the `.map(...)`. Check with:

```bash
LC_ALL=C grep -an "fn paths" apps/windows/src/library.rs
```

- [ ] **Step 6: Run everything**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. The seven moved tests now run under `pe-session` instead of `pe-windows`; the total count across the workspace should not drop.

- [ ] **Step 7: Commit**

```bash
git add -A crates/pe-session apps/windows
git commit -m "Never write over an original becomes a rule the Mac gets too"
```

---

## Task 6: The session renders a photograph

The aggregate the shells talk to: one open photograph, its edit, its history,
and the GPU objects that turn the two into pixels.

**Note on scope.** `apps/windows` is *not* converted onto this aggregate. Its
`App` struct is entangled with egui's frame loop by design, and rewriting it
onto `Session` would risk a working application for no gain — the shared
*rules* already moved in Tasks 4 and 5, which is what the spec asked for. This
type is new code, for the Swift shells.

**Files:**
- Create: `crates/pe-session/src/session.rs`
- Modify: `crates/pe-session/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/pe-session/src/session.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mean(pixels: &[u8]) -> f32 {
        // Alpha excluded — it is 255 everywhere and would flatten the answer.
        let sum: u64 = pixels
            .chunks_exact(4)
            .flat_map(|p| p[..3].iter().map(|c| u64::from(*c)))
            .sum();
        sum as f32 / (pixels.len() as f32 / 4.0 * 3.0)
    }

    fn chart_session() -> Session {
        let mut s = Session::new();
        s.open_test_chart(256, 256).expect("a test chart always opens");
        s
    }

    #[test]
    fn a_fresh_session_has_nothing_open() {
        let s = Session::new();
        assert!(!s.is_open());
        assert_eq!(s.row_count(), 0);
    }

    #[test]
    fn opening_a_chart_gives_it_the_pinned_rows() {
        let s = chart_session();
        assert!(s.is_open());
        // new_document seeds the pinned rows the inspector shows as fixed
        // panels, so an opened photograph is never an empty stack.
        assert_eq!(s.row_count(), pe_effects::PINNED_ROWS.len());
    }

    #[test]
    fn exposure_makes_the_picture_brighter() {
        let mut s = chart_session();
        let before = mean(&s.render_offscreen(256, 256).unwrap());

        let row = s.add_effect("exposure").expect("exposure is a registered effect");
        s.set_float(row, "ev", 2.0).unwrap();

        let after = mean(&s.render_offscreen(256, 256).unwrap());
        assert!(
            after > before + 5.0,
            "two stops did nothing: {before} -> {after}"
        );
    }

    #[test]
    fn an_unchanged_document_costs_no_passes() {
        let mut s = chart_session();
        s.render_offscreen(256, 256).unwrap();
        s.render_offscreen(256, 256).unwrap();
        assert_eq!(s.last_passes(), 0, "re-rendered something that had not changed");
    }

    #[test]
    fn moving_one_slider_in_a_deep_stack_costs_one_pass() {
        // The number the toolbar shows, and the reason the application does not
        // get slower as you do more to an image. If this ever reads the stack
        // depth, the stage cache has stopped working.
        let mut s = chart_session();
        let mut deepest = None;
        for _ in 0..4 {
            deepest = Some(s.add_effect("exposure").unwrap());
        }
        s.render_offscreen(256, 256).unwrap();

        s.set_float(deepest.unwrap(), "ev", 0.5).unwrap();
        s.render_offscreen(256, 256).unwrap();
        assert_eq!(s.last_passes(), 1);
    }

    #[test]
    fn a_parameter_that_is_not_there_is_refused_rather_than_invented() {
        let mut s = chart_session();
        let row = s.add_effect("exposure").unwrap();
        assert!(s.set_float(row, "not_a_parameter", 1.0).is_err());
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — `Session` unresolved.

- [ ] **Step 3: Write the implementation**

Put this above the test module in `crates/pe-session/src/session.rs`:

```rust
//! The open photograph, its edit, and the GPU objects between them.
//!
//! What a shell talks to. It reads the stack, mutates a parameter, asks for a
//! frame, and draws it — which is the same vocabulary `apps/windows` has, with
//! the parts that were never about interface moved down here where the Mac and
//! the iPad can reach them.

use std::path::{Path, PathBuf};

use pe_color::space;
use pe_core::{Document, Geometry, History, ParamValue, RowId, RowIdGenerator, StackRow};
use pe_io::DecodedImage;
use pe_render::{
    EffectRenderer, GpuContext, ImageTexture, Region, Sampling, TransformPass,
};

use crate::surface::Attached;
use crate::{Support, autosave, export};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("nothing is open")]
    NothingOpen,
    #[error("no GPU: {0}")]
    NoGpu(String),
    #[error("could not read {path}: {message}")]
    Read { path: String, message: String },
    #[error("no row with id {0}")]
    NoSuchRow(u64),
    #[error("{effect} has no parameter called {key}")]
    NoSuchParam { effect: String, key: String },
    #[error("{0} is not a registered effect")]
    NoSuchEffect(String),
    #[error("render failed: {0}")]
    Render(String),
    #[error("refused: {0} is one of your photographs")]
    WouldOverwriteSource(String),
    #[error("write failed: {0}")]
    Write(String),
    #[error("no layer attached")]
    NoLayer,
}

/// The photograph that is open, and its edit.
struct Photo {
    path: Option<PathBuf>,
    image: DecodedImage,
    history: History,
    ids: RowIdGenerator,
}

/// Everything that needs a device. Built lazily, because a session exists
/// before a window does and a headless test never needs a surface.
#[derive(Default)]
struct Gpu {
    context: Option<GpuContext>,
    attached: Option<Attached>,
    renderer: Option<EffectRenderer>,
    to_working: Option<TransformPass>,
    to_display: Option<TransformPass>,
    source: Option<ImageTexture>,
    working: Option<ImageTexture>,
    working_size: (u32, u32),
    working_geometry: Option<Geometry>,
    last_passes: usize,
}

pub struct Session {
    instance: wgpu::Instance,
    gpu: Gpu,
    photo: Option<Photo>,
    support: Support,
    export_settings: export::Export,
    watcher: autosave::Watcher,
    /// Bumped by every mutation, so a shell can ask "is this still what I last
    /// saw?" with one integer instead of a JSON parse.
    snapshot_version: u64,
    /// Set while a drag is in progress. Consecutive edits sharing it collapse
    /// into one undo step. See `History::edit`.
    interaction: Option<String>,
    needs_render: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            instance: GpuContext::create_instance(),
            gpu: Gpu::default(),
            photo: None,
            support: Support::default(),
            export_settings: export::Export::default(),
            watcher: autosave::Watcher::new(),
            snapshot_version: 0,
            interaction: None,
            needs_render: true,
        }
    }

    /// Where this host keeps the application's own files. See [`Support`].
    pub fn set_support_dir(&mut self, root: impl Into<PathBuf>) {
        self.support = Support::at(root);
    }

    pub fn is_open(&self) -> bool {
        self.photo.is_some()
    }

    pub fn last_passes(&self) -> usize {
        self.gpu.last_passes
    }

    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    pub fn snapshot_version(&self) -> u64 {
        self.snapshot_version
    }

    pub fn document(&self) -> Option<&Document> {
        Some(self.photo.as_ref()?.history.document())
    }

    pub fn row_count(&self) -> usize {
        self.document().map_or(0, |d| d.stack.len())
    }

    pub fn path(&self) -> Option<&Path> {
        self.photo.as_ref()?.path.as_deref()
    }

    // ---- opening --------------------------------------------------------

    /// Open a photograph, restoring whatever was being done to it last time.
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let path = path.as_ref().to_path_buf();
        let image = pe_io::load(&path).map_err(|e| SessionError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        // The autosave wins over a fresh document, because it is where the
        // person happened to stop. A sidecar is pulled over the top explicitly.
        let doc = autosave::load(&self.support, &path)
            .unwrap_or_else(|| pe_effects::new_document(path.to_string_lossy()));
        self.adopt(Some(path), image, doc);
        Ok(())
    }

    /// Open the built-in chart, for a session with no file behind it.
    pub fn open_test_chart(&mut self, width: u32, height: u32) -> Result<(), SessionError> {
        let image = pe_io::test_chart(width, height);
        let doc = pe_effects::new_document("test-chart");
        self.adopt(None, image, doc);
        Ok(())
    }

    fn adopt(&mut self, path: Option<PathBuf>, image: DecodedImage, doc: Document) {
        // A source the file itself declared beats the document's guess, which
        // is how a Display P3 file from a phone renders as Display P3.
        let mut doc = doc;
        if let Some(declared) = image.space {
            doc.color.input = declared.to_string();
        }
        let ids = RowIdGenerator::resuming(&doc);
        let history = History::new(doc);
        self.watcher.reset(history.revision());
        self.photo = Some(Photo {
            path,
            image,
            history,
            ids,
        });
        // Every cached stage and both intermediates belong to the old picture.
        self.gpu.source = None;
        self.gpu.working = None;
        self.gpu.working_size = (0, 0);
        self.gpu.working_geometry = None;
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.touched();
    }

    fn touched(&mut self) {
        self.snapshot_version += 1;
        self.needs_render = true;
    }

    // ---- editing --------------------------------------------------------

    /// Bracket a drag so it becomes one undo step rather than three hundred.
    pub fn begin_interaction(&mut self, label: impl Into<String>) {
        self.interaction = Some(label.into());
    }

    pub fn end_interaction(&mut self) {
        self.interaction = None;
        if let Some(p) = self.photo.as_mut() {
            p.history.break_coalescing();
        }
    }

    fn edit<F>(&mut self, label: &str, f: F) -> Result<(), SessionError>
    where
        F: FnOnce(&mut Document),
    {
        let coalesce = self.interaction.clone();
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        photo.history.edit(label, coalesce, f);
        self.touched();
        Ok(())
    }

    pub fn add_effect(&mut self, key: &str) -> Result<RowId, SessionError> {
        let def = pe_effects::by_key(key)
            .ok_or_else(|| SessionError::NoSuchEffect(key.to_string()))?;
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let id = photo.ids.allocate();
        let params = def.default_params();
        self.edit(&format!("Add {}", def.name), move |doc| {
            let mut row = StackRow::new(id, key);
            row.params = params;
            doc.stack.push(row);
        })?;
        Ok(id)
    }

    pub fn remove_row(&mut self, id: RowId) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Remove row", move |doc| {
            doc.stack.remove(id);
        })
    }

    pub fn move_row(&mut self, id: RowId, to: usize) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Reorder", move |doc| {
            doc.stack.reorder(id, to);
        })
    }

    pub fn set_row_enabled(&mut self, id: RowId, on: bool) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Enable row", move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.enabled = on;
            }
        })
    }

    pub fn set_row_opacity(&mut self, id: RowId, value: f32) -> Result<(), SessionError> {
        self.require_row(id)?;
        self.edit("Opacity", move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.opacity = value.clamp(0.0, 1.0);
            }
        })
    }

    /// Set a parameter, refusing one the effect does not declare.
    ///
    /// Refused rather than inserted: a typo that silently adds a key produces
    /// a document with a parameter no shader reads and no UI shows, which is
    /// indistinguishable from the slider being broken.
    pub fn set_param(
        &mut self,
        id: RowId,
        key: &str,
        value: ParamValue,
    ) -> Result<(), SessionError> {
        let effect = self.require_row(id)?;
        let def = pe_effects::by_key(&effect)
            .ok_or_else(|| SessionError::NoSuchEffect(effect.clone()))?;
        let param = def
            .params
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| SessionError::NoSuchParam {
                effect: effect.clone(),
                key: key.to_string(),
            })?;
        let label = param.name.to_string();
        let key = key.to_string();
        self.edit(&label, move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.params.set(key, value);
            }
        })
    }

    pub fn set_float(&mut self, id: RowId, key: &str, value: f32) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Float(value))
    }

    pub fn set_bool(&mut self, id: RowId, key: &str, value: bool) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Bool(value))
    }

    pub fn set_choice(&mut self, id: RowId, key: &str, value: &str) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Choice(value.to_string()))
    }

    pub fn set_rgb(&mut self, id: RowId, key: &str, value: [f32; 3]) -> Result<(), SessionError> {
        self.set_param(id, key, ParamValue::Rgb(value))
    }

    /// The effect key of the row, or an error naming the id that was missing.
    fn require_row(&self, id: RowId) -> Result<String, SessionError> {
        self.document()
            .ok_or(SessionError::NothingOpen)?
            .stack
            .get(id)
            .map(|r| r.effect.clone())
            .ok_or(SessionError::NoSuchRow(id.0))
    }

    pub fn undo(&mut self) -> Result<bool, SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let moved = photo.history.undo();
        if moved {
            self.touched();
        }
        Ok(moved)
    }

    pub fn redo(&mut self) -> Result<bool, SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let moved = photo.history.redo();
        if moved {
            self.touched();
        }
        Ok(moved)
    }

    pub fn can_undo(&self) -> bool {
        self.photo.as_ref().is_some_and(|p| p.history.can_undo())
    }

    pub fn can_redo(&self) -> bool {
        self.photo.as_ref().is_some_and(|p| p.history.can_redo())
    }

    // ---- rendering ------------------------------------------------------

    fn context(&mut self) -> Result<&GpuContext, SessionError> {
        if self.gpu.context.is_none() {
            let gpu = pollster::block_on(GpuContext::from_instance(&self.instance, None))
                .map_err(|e| SessionError::NoGpu(e.to_string()))?;
            self.gpu.context = Some(gpu);
        }
        Ok(self.gpu.context.as_ref().expect("built above"))
    }

    /// Run the stack, returning the graded texture in the working space.
    fn graded(&mut self, width: u32, height: u32) -> Result<ImageTexture, SessionError> {
        self.context()?;
        let gpu = self.gpu.context.as_ref().expect("context built above");
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let doc = photo.history.document();

        if self.gpu.source.is_none() {
            self.gpu.source = Some(
                ImageTexture::upload_rgba8(
                    &gpu.device,
                    &gpu.queue,
                    photo.image.width,
                    photo.image.height,
                    &photo.image.pixels,
                    "source",
                )
                .map_err(|e| SessionError::Render(e.to_string()))?,
            );
        }
        if self.gpu.to_working.is_none() {
            self.gpu.to_working = Some(TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT));
            self.gpu.to_display = Some(TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT));
            self.gpu.renderer = Some(EffectRenderer::new(&gpu.device));
        }

        let geometry = doc.geometry;
        if self.gpu.working_size != (width, height) || self.gpu.working_geometry != Some(geometry) {
            let sampling = Sampling::of(&geometry, photo.image.width, photo.image.height)
                .within(Region::FULL);
            let source = self.gpu.source.as_ref().expect("uploaded above");
            self.gpu.working = Some(self.gpu.to_working.as_ref().expect("built above").
                to_working_mapped(
                    gpu,
                    source,
                    &doc.color.pipeline().input,
                    width,
                    height,
                    sampling,
                ));
            self.gpu.working_size = (width, height);
            self.gpu.working_geometry = Some(geometry);
        }

        let working = self.gpu.working.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_mut().expect("built above");
        renderer.set_region(Region::FULL);
        let graded = renderer.render(gpu, working, doc, 1);
        self.gpu.last_passes = renderer.last_pass_count();
        Ok(ImageTexture {
            view: graded.view.clone(),
            ..ImageTexture::clone_handle(graded)
        })
    }

    /// Render at `width`×`height` and read the result back as RGBA8.
    ///
    /// Used by the tests and, later, by the thumbnail path. The interactive
    /// route is [`Session::present`], which writes to the attached layer and
    /// never stalls the GPU reading anything back.
    pub fn render_offscreen(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, SessionError> {
        let graded_view = self.graded(width, height)?.view;
        let gpu = self.gpu.context.as_ref().expect("built by graded");
        let output = self
            .photo
            .as_ref()
            .expect("graded checked this")
            .history
            .document()
            .color
            .pipeline()
            .output;

        let target = ImageTexture::new(
            &gpu.device,
            width,
            height,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            "offscreen",
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("offscreen"),
            });
        self.gpu.to_display.as_ref().expect("built by graded").encode(
            gpu,
            &mut encoder,
            &graded_view,
            &target.view,
            &space::ACESCG,
            &output,
        );
        gpu.queue.submit([encoder.finish()]);
        self.needs_render = false;
        pe_render::read_rgba8(gpu, &target).map_err(|e| SessionError::Render(e.to_string()))
    }
}
```

Add to `crates/pe-session/src/lib.rs`:

```rust
pub mod session;

pub use session::{Session, SessionError};
```

- [ ] **Step 4: Fix the texture-handle problem the compiler will point at**

`EffectRenderer::render` returns `&ImageTexture` borrowed from the renderer, and `graded()` cannot return a borrow while also mutating `self`. `ImageTexture::clone_handle` does not exist — that line in Step 3 is a placeholder the compiler will reject.

Replace the end of `graded()` with a version that returns only the view, which is all any consumer needs and is cheaply cloneable:

```rust
        let working = self.gpu.working.as_ref().expect("built above");
        let renderer = self.gpu.renderer.as_mut().expect("built above");
        renderer.set_region(Region::FULL);
        let graded = renderer.render(gpu, working, doc, 1);
        let view = graded.view.clone();
        self.gpu.last_passes = renderer.last_pass_count();
        Ok(view)
    }
```

and change its signature to `fn graded(&mut self, width: u32, height: u32) -> Result<wgpu::TextureView, SessionError>`, and in `render_offscreen` change `let graded_view = self.graded(width, height)?.view;` to `let graded_view = self.graded(width, height)?;`.

If the borrow checker still objects to `gpu` being borrowed from `self.gpu.context` while `self.gpu.renderer` is mutably borrowed, take the context out for the duration:

```rust
        let gpu = self.gpu.context.take().expect("built above");
        // ... use &gpu throughout ...
        self.gpu.context = Some(gpu);
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 23 tests. The GPU ones need an adapter; on a Mac that is Metal and always present.

If `moving_one_slider_in_a_deep_stack_costs_one_pass` reports more than 1, the stage cache is being invalidated by something the session does per frame — most likely `Region` or the geometry fingerprint changing. Compare against `apps/windows/src/preview.rs`'s `rebuild` guard, which only rebuilds when size, region or geometry actually differ.

- [ ] **Step 6: Commit**

```bash
git add -A crates/pe-session
git commit -m "A session that opens a photograph, grades it, and re-runs only what moved"
```

---

## Task 7: Present to the attached layer

The offscreen path proved the stack runs. This is the same render going to the
screen instead of to a buffer.

**Files:**
- Modify: `crates/pe-session/src/session.rs`

- [ ] **Step 1: Write the failing test**

A surface cannot be made in a unit test, so what is testable is the refusal.
Add to the test module in `session.rs`:

```rust
    #[test]
    fn presenting_without_a_layer_says_so_rather_than_crashing() {
        let mut s = chart_session();
        assert!(matches!(s.present(), Err(SessionError::NoLayer)));
    }

    #[test]
    fn attaching_a_null_layer_is_refused() {
        let mut s = Session::new();
        let rc = unsafe { s.attach_layer(std::ptr::null_mut(), 100, 100) };
        assert!(rc.is_err());
    }
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — no method `present`, no method `attach_layer`.

- [ ] **Step 3: Write the implementation**

Add to `impl Session` in `session.rs`:

```rust
    // ---- the screen -----------------------------------------------------

    /// Adopt a `CAMetalLayer` the host owns.
    ///
    /// # Safety
    /// `layer` must be a live `CAMetalLayer` that outlives the attachment. The
    /// Swift side guarantees this by holding it on a view it owns and calling
    /// [`Session::detach_layer`] before that view goes away.
    pub unsafe fn attach_layer(
        &mut self,
        layer: *mut std::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<(), SessionError> {
        if layer.is_null() {
            return Err(SessionError::NoLayer);
        }
        // The adapter must come from the instance the surface belongs to, and
        // must be told about the surface: on a machine with more than one GPU,
        // the one picked otherwise may not be able to present to this window.
        let probe = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        }
        .map_err(|e| SessionError::NoGpu(e.to_string()))?;
        if self.gpu.context.is_none() {
            let gpu = pollster::block_on(GpuContext::from_instance(&self.instance, Some(&probe)))
                .map_err(|e| SessionError::NoGpu(e.to_string()))?;
            self.gpu.context = Some(gpu);
        }
        drop(probe);

        let gpu = self.gpu.context.as_ref().expect("built above");
        let attached = unsafe {
            Attached::new(
                &self.instance,
                &gpu.adapter,
                &gpu.device,
                layer,
                width,
                height,
            )
        }
        .map_err(SessionError::NoGpu)?;
        self.gpu.attached = Some(attached);
        self.needs_render = true;
        Ok(())
    }

    pub fn resize_layer(&mut self, width: u32, height: u32) {
        if let (Some(a), Some(gpu)) = (self.gpu.attached.as_mut(), self.gpu.context.as_ref()) {
            a.resize(&gpu.device, width, height);
            // The working texture was built for the old size, and so was every
            // cached stage that reads it.
            self.gpu.working_size = (0, 0);
            self.needs_render = true;
        }
    }

    pub fn detach_layer(&mut self) {
        self.gpu.attached = None;
    }

    /// Draw the current state into the attached layer and present it.
    pub fn present(&mut self) -> Result<(), SessionError> {
        let (width, height) = match self.gpu.attached.as_ref() {
            Some(a) => (a.config.width, a.config.height),
            None => return Err(SessionError::NoLayer),
        };
        if self.photo.is_none() {
            // Nothing open yet — the viewer's background, not an error.
            let gpu = self.context()?;
            let attached = self.gpu.attached.as_ref().expect("checked above");
            return attached
                .present_clear(&gpu.device, &gpu.queue, [0.06, 0.06, 0.07, 1.0])
                .map_err(SessionError::Render);
        }

        let graded_view = self.graded(width, height)?;
        let output = self
            .photo
            .as_ref()
            .expect("checked above")
            .history
            .document()
            .color
            .pipeline()
            .output;
        let gpu = self.gpu.context.as_ref().expect("built by graded");
        let attached = self.gpu.attached.as_ref().expect("checked above");

        let frame = attached
            .surface
            .get_current_texture()
            .map_err(|e| SessionError::Render(e.to_string()))?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present"),
            });
        self.gpu
            .to_display
            .as_ref()
            .expect("built by graded")
            .encode(gpu, &mut encoder, &graded_view, &view, &space::ACESCG, &output);
        gpu.queue.submit([encoder.finish()]);
        frame.present();
        self.needs_render = false;
        Ok(())
    }
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session && cargo clippy -p pe-session --all-targets -- -D warnings
```

Expected: PASS, 25 tests, no clippy output.

- [ ] **Step 5: Commit**

```bash
git add -A crates/pe-session
git commit -m "The graded frame goes to the layer the host gave us"
```

---

## Task 8: Autosave, sidecars and export, on the session

The rules from Tasks 4 and 5, wired to the open photograph.

**Files:**
- Modify: `crates/pe-session/src/session.rs`

- [ ] **Step 1: Write the failing tests**

Add to the test module in `session.rs`:

```rust
    #[test]
    fn work_in_progress_comes_back_when_the_photograph_is_reopened() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.png");
        let chart = pe_io::test_chart(64, 64);
        pe_io::save_png(&chart, &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_path(&photo).unwrap();
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 1.5).unwrap();
        s.write_autosave();

        let mut again = Session::new();
        again.set_support_dir(tmp.path().join("support"));
        again.open_path(&photo).unwrap();
        let doc = again.document().unwrap();
        let restored = doc
            .stack
            .iter()
            .find(|r| r.effect == "exposure")
            .and_then(|r| r.params.get("ev"))
            .and_then(|v| v.as_float());
        assert_eq!(restored, Some(1.5));
    }

    #[test]
    fn reverting_leaves_nothing_to_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("a.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.set_support_dir(tmp.path().join("support"));
        s.open_path(&photo).unwrap();
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "ev", 1.5).unwrap();
        s.write_autosave();
        s.revert().unwrap();

        let mut again = Session::new();
        again.set_support_dir(tmp.path().join("support"));
        again.open_path(&photo).unwrap();
        assert!(
            again.document().unwrap().stack.iter().all(|r| r.effect != "exposure"),
            "the reverted edit came back"
        );
    }

    #[test]
    fn an_export_is_written_beside_the_original_with_the_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("sunset.png");
        pe_io::save_png(&pe_io::test_chart(64, 64), &photo, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        let out = s.export_current().unwrap();
        assert_eq!(out.file_name().unwrap(), "sunset_KROMA.jpg");
        assert!(out.exists());
        // And the original is untouched.
        assert!(photo.exists());
    }

    #[test]
    fn an_export_that_would_land_on_an_original_is_refused() {
        // Contrived deliberately: a file already named as an export. Opening it
        // and exporting must not write over it.
        let tmp = tempfile::tempdir().unwrap();
        let photo = tmp.path().join("sunset_KROMA.jpg");
        let chart = pe_io::test_chart(64, 64);
        pe_io::save_jpeg(&chart, &photo, 95, &pe_color::space::SRGB).unwrap();

        let mut s = Session::new();
        s.open_path(&photo).unwrap();
        // Its export would be sunset_KROMA_KROMA.jpg, which is safe — so to
        // exercise the refusal, claim the output name is one of ours.
        s.set_open_set(vec![tmp.path().join("sunset_KROMA_KROMA.jpg")]);
        assert!(matches!(
            s.export_current(),
            Err(SessionError::WouldOverwriteSource(_))
        ));
    }
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — no `write_autosave`, `revert`, `export_current`, `set_open_set`.

- [ ] **Step 3: Write the implementation**

Add a field to `struct Session`, after `export_settings`:

```rust
    /// Every photograph currently open, for the collision check. The one on
    /// screen is in here too. A batch writes into one folder and the name it
    /// builds for photo A can collide with photo B sitting right beside it.
    open_set: Vec<PathBuf>,
```

initialised to `Vec::new()` in `Session::new`. Add to `impl Session`:

```rust
    // ---- persistence ----------------------------------------------------

    /// Every photograph the collision check must consider.
    pub fn set_open_set(&mut self, paths: Vec<PathBuf>) {
        self.open_set = paths;
    }

    /// Called every frame. Writes the work in progress once the user has
    /// stopped moving. See [`autosave::Watcher`].
    pub fn tick(&mut self) {
        let Some(photo) = self.photo.as_ref() else {
            return;
        };
        let revision = photo.history.revision();
        if self.watcher.tick(revision, std::time::Instant::now()) {
            self.write_autosave();
        }
    }

    /// Write the work in progress now, throttle or no throttle.
    ///
    /// Called when leaving a photograph, where the throttle is beside the
    /// point: the thing that would have triggered the write is about to stop
    /// being the thing on screen.
    pub fn write_autosave(&mut self) {
        let Some(photo) = self.photo.as_ref() else {
            return;
        };
        let Some(path) = photo.path.as_ref() else {
            return;
        };
        autosave::store(&self.support, path, photo.history.document());
        let revision = photo.history.revision();
        self.watcher.reset(revision);
    }

    /// The explicit save: a `.peproj` beside the photograph.
    ///
    /// A sidecar is a decision — *this* is the edit, keep it, move it with the
    /// photograph. The autosave is just where you happened to stop.
    pub fn save_sidecar(&mut self) -> Result<PathBuf, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let path = photo.path.as_ref().ok_or(SessionError::NothingOpen)?;
        let out = path.with_extension("peproj");
        let json = photo
            .history
            .document()
            .to_json()
            .map_err(|e| SessionError::Write(e.to_string()))?;
        pe_io::write_bytes_atomically(&out, json.as_bytes())
            .map_err(|e| SessionError::Write(e.to_string()))?;
        Ok(out)
    }

    /// Pull a sidecar back over the top of whatever is showing.
    pub fn load_sidecar(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(|e| SessionError::Read {
            path: path.as_ref().display().to_string(),
            message: e.to_string(),
        })?;
        let doc = Document::from_json(&text).map_err(|e| SessionError::Read {
            path: path.as_ref().display().to_string(),
            message: e.to_string(),
        })?;
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        photo.ids = RowIdGenerator::resuming(&doc);
        photo.history.edit("Load edit", None, move |d| *d = doc);
        self.gpu.working_geometry = None;
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.touched();
        Ok(())
    }

    /// Throw the edit and the saved work away.
    ///
    /// An edit that comes back every time you open a photograph, with no way
    /// to be rid of it, is not a convenience — it is a photograph you can no
    /// longer see.
    pub fn revert(&mut self) -> Result<(), SessionError> {
        let photo = self.photo.as_mut().ok_or(SessionError::NothingOpen)?;
        let source = photo
            .path
            .clone()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "test-chart".to_string());
        let fresh = pe_effects::new_document(source);
        photo.ids = RowIdGenerator::resuming(&fresh);
        photo.history.edit("Revert", None, move |d| *d = fresh);
        if let Some(path) = photo.path.clone() {
            autosave::forget(&self.support, &path);
        }
        let revision = self
            .photo
            .as_ref()
            .expect("still open")
            .history
            .revision();
        self.watcher.reset(revision);
        if let Some(r) = self.gpu.renderer.as_mut() {
            r.invalidate();
        }
        self.gpu.working_geometry = None;
        self.touched();
        Ok(())
    }

    // ---- export ---------------------------------------------------------

    pub fn set_export(&mut self, format: export::Format, quality: u8) {
        self.export_settings = export::Export {
            format,
            quality: quality.clamp(1, 100),
        };
    }

    pub fn export_settings(&self) -> export::Export {
        self.export_settings
    }

    /// Write the graded photograph beside its original, refusing a collision.
    pub fn export_current(&mut self) -> Result<PathBuf, SessionError> {
        let photo = self.photo.as_ref().ok_or(SessionError::NothingOpen)?;
        let source = photo
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("export"));
        let chosen = self.export_settings;
        let out = source.with_file_name(export::export_name(&source, chosen.format));

        // Both defences, in order. The naming keeps them apart; the check is
        // what makes it a guarantee rather than a scheme that happens to work.
        let mut open = self.open_set.clone();
        open.push(source.clone());
        if export::would_overwrite_a_source(&open, &out) {
            return Err(SessionError::WouldOverwriteSource(out.display().to_string()));
        }

        self.context()?;
        let gpu = self.gpu.context.as_ref().expect("built above");
        if self.gpu.renderer.is_none() {
            self.gpu.renderer = Some(EffectRenderer::new(&gpu.device));
        }
        let renderer = self.gpu.renderer.as_ref().expect("built above");
        let photo = self.photo.as_ref().expect("checked above");
        let doc = photo.history.document();
        let (w, h) = pe_render::export::output_size(doc, photo.image.width, photo.image.height);
        // The space the pipeline actually rendered to, which is what the file
        // has to say it is in. Taken from the same settings the render read, so
        // the two cannot disagree — a file labelled with anything else is a
        // wrong answer stated confidently, and every reader will believe it.
        let out_space = doc.color.pipeline().output;

        if chosen.format.is_sixteen_bit() {
            let pixels = pe_render::export::render_full_16(
                gpu,
                renderer,
                photo.image.width,
                photo.image.height,
                &photo.image.pixels,
                doc,
            )
            .map_err(|e| SessionError::Render(e.to_string()))?;
            pe_io::save_png16(w, h, &pixels, &out, &out_space)
                .map_err(|e| SessionError::Write(e.to_string()))?;
        } else {
            let pixels = pe_render::render_full(
                gpu,
                renderer,
                photo.image.width,
                photo.image.height,
                &photo.image.pixels,
                doc,
            )
            .map_err(|e| SessionError::Render(e.to_string()))?;
            let img = pe_io::DecodedImage::new(w, h, pixels)
                .map_err(|e| SessionError::Write(e.to_string()))?;
            match chosen.format {
                export::Format::Jpeg => {
                    pe_io::save_jpeg(&img, &out, chosen.quality, &out_space)
                }
                _ => pe_io::save_png(&img, &out, &out_space),
            }
            .map_err(|e| SessionError::Write(e.to_string()))?;
        }
        Ok(out)
    }
```

`render_full` and `render_full_16` take `&EffectRenderer`. If the compiler says
they want `&mut`, change the two `as_ref()` calls above to `as_mut()`.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 29 tests.

- [ ] **Step 5: Commit**

```bash
git add -A crates/pe-session
git commit -m "Stopping costs nothing, a sidecar is a decision, and an export never lands on a photograph"
```

---

## Task 9: The registry, as data Swift can read

The Windows inspector is one `match` on `ParamKind` covering all thirty
effects. This is what lets the Swift inspector be the same size: ship the
registry as JSON, implement eight control views, and every effect — including
ones added later — appears with no Swift changes.

**Files:**
- Create: `crates/pe-session/src/describe.rs`
- Create: `crates/pe-session/tests/fixtures.rs`
- Create: `apps/apple/Fixtures/registry.json`
- Modify: `crates/pe-session/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/pe-session/src/describe.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_effect_is_described() {
        let r = registry();
        assert_eq!(r.effects.len(), pe_effects::all().len());
        assert_eq!(r.pinned, pe_effects::PINNED_ROWS);
    }

    #[test]
    fn a_float_parameter_carries_everything_a_slider_needs() {
        let r = registry();
        let exposure = r.effects.iter().find(|e| e.key == "exposure").unwrap();
        let ev = exposure.params.iter().find(|p| p.key == "ev").unwrap();
        assert_eq!(ev.kind, "float");
        // Without all four a slider cannot draw itself: where it starts, where
        // it ends, where it rests, and where its fill grows from.
        assert!(ev.min.is_some());
        assert!(ev.max.is_some());
        assert!(ev.default_float.is_some());
        assert!(ev.neutral.is_some());
    }

    #[test]
    fn every_param_kind_survives_the_crossing() {
        // Eight kinds, eight control views on the Swift side. A kind that
        // serialises as something Swift cannot name is a control that silently
        // does not appear.
        let r = registry();
        let kinds: std::collections::BTreeSet<&str> = r
            .effects
            .iter()
            .flat_map(|e| e.params.iter().map(|p| p.kind.as_str()))
            .collect();
        for expected in ["float", "bool", "rgb", "wheel", "curve", "choice", "pins", "warp"] {
            assert!(kinds.contains(expected), "no parameter serialised as {expected}");
        }
    }

    #[test]
    fn a_choice_lists_its_options() {
        let r = registry();
        let choice = r
            .effects
            .iter()
            .flat_map(|e| e.params.iter())
            .find(|p| p.kind == "choice")
            .expect("some effect has a dropdown");
        assert!(!choice.options.is_empty());
        assert!(choice.default_choice.is_some());
    }

    #[test]
    fn gates_travel_with_the_effect_that_owns_them() {
        let r = registry();
        let gated = r
            .effects
            .iter()
            .find(|e| !e.gates.is_empty())
            .expect("some effect greys out controls");
        let g = &gated.gates[0];
        assert!(!g.by.is_empty());
        assert!(!g.params.is_empty());
        assert!(["true", "false", "positive", "is", "drawn"].contains(&g.when.as_str()));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — `registry` unresolved.

- [ ] **Step 3: Write the implementation**

Put this above the test module in `crates/pe-session/src/describe.rs`:

```rust
//! The registry and the open document, as JSON, for a front end that cannot
//! see Rust types.
//!
//! `pe-effects`'s own types are `&'static str` and cannot derive `Serialize`,
//! and should not: the registry is a compile-time table, and making it
//! serialisable to suit one consumer would put a serde attribute on every line
//! of a 4,700-line file. These are flat mirrors, built on demand.
//!
//! Flat on purpose. A tagged union with associated values is pleasant in Rust
//! and in Swift and unpleasant in the JSON between them, where every optional
//! field costs one `if let` on each side. `kind` is a string and the fields
//! that apply to that kind are set; the rest are absent.

use serde::{Deserialize, Serialize};

use pe_effects::{Gate, ParamKind, When};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    /// Every effect, in the order the browser lists them.
    pub effects: Vec<Effect>,
    /// The rows a fresh document starts with, as fixed panels.
    pub pinned: Vec<String>,
    /// Effects that do something visible at their defaults, so the UI can warn
    /// that adding one changes the picture immediately.
    pub visible_at_defaults: Vec<String>,
    /// Group headings, in the order the browser shows them.
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub key: String,
    pub name: String,
    pub group: String,
    /// `"linear"` or `"log"` — which working space the renderer runs it in.
    /// The UI never acts on this; it is here for the About panel and for bug
    /// reports, where "which space was that in" is the first question.
    pub space: String,
    pub spatial: bool,
    pub params: Vec<Param>,
    pub gates: Vec<GateJson>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Param {
    pub key: String,
    pub name: String,
    /// One of: float, bool, rgb, wheel, curve, choice, pins, warp.
    pub kind: String,
    pub unit: String,
    pub section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_float: Option<f32>,
    /// Where "no change" sits — usually but not always the default. Sliders
    /// draw their fill from here and double-click resets to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neutral: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_bool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_rgb: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_choice: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Wheels only: whether there is a fourth, achromatic readout. Not whether
    /// there is an achromatic control — every wheel has the ribbed bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master: Option<bool>,
    /// Curves only: whether the identity is a flat line rather than the
    /// diagonal. Getting this wrong rotates every hue in the picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateJson {
    pub by: String,
    /// One of: true, false, positive, is, drawn.
    pub when: String,
    /// Set only when `when` is `is`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    pub params: Vec<String>,
}

fn param(p: &pe_effects::ParamDef) -> Param {
    let mut out = Param {
        key: p.key.to_string(),
        name: p.name.to_string(),
        kind: String::new(),
        unit: p.unit.to_string(),
        section: p.section.to_string(),
        min: None,
        max: None,
        default_float: None,
        neutral: None,
        default_bool: None,
        default_rgb: None,
        default_choice: None,
        options: Vec::new(),
        master: None,
        flat: None,
    };
    match p.kind {
        ParamKind::Float {
            min,
            max,
            default,
            neutral,
        } => {
            out.kind = "float".into();
            out.min = Some(min);
            out.max = Some(max);
            out.default_float = Some(default);
            out.neutral = Some(neutral);
        }
        ParamKind::Bool { default } => {
            out.kind = "bool".into();
            out.default_bool = Some(default);
        }
        ParamKind::Rgb { default } => {
            out.kind = "rgb".into();
            out.default_rgb = Some(default);
        }
        ParamKind::Wheel {
            min,
            max,
            default,
            master,
        } => {
            out.kind = "wheel".into();
            out.min = Some(min);
            out.max = Some(max);
            out.default_float = Some(default);
            out.neutral = Some(default);
            out.master = Some(master);
        }
        ParamKind::Curve { flat } => {
            out.kind = "curve".into();
            out.flat = Some(flat);
        }
        ParamKind::Choice { options, default } => {
            out.kind = "choice".into();
            out.options = options.iter().map(|o| o.to_string()).collect();
            out.default_choice = Some(default.to_string());
        }
        ParamKind::Pins => out.kind = "pins".into(),
        ParamKind::Warp => out.kind = "warp".into(),
    }
    out
}

fn gate(g: &Gate) -> GateJson {
    let (when, option) = match g.when {
        When::True => ("true", None),
        When::False => ("false", None),
        When::Positive => ("positive", None),
        When::Is(o) => ("is", Some(o.to_string())),
        When::Drawn => ("drawn", None),
    };
    GateJson {
        by: g.by.to_string(),
        when: when.to_string(),
        option,
        params: g.params.iter().map(|p| p.to_string()).collect(),
    }
}

/// The whole registry, ready to hand to a front end.
pub fn registry() -> Registry {
    Registry {
        effects: pe_effects::all()
            .iter()
            .map(|e| Effect {
                key: e.key.to_string(),
                name: e.name.to_string(),
                group: e.group.as_str().to_string(),
                space: match e.space {
                    pe_color::WorkingSpace::Linear => "linear",
                    pe_color::WorkingSpace::Log => "log",
                }
                .to_string(),
                spatial: e.spatial,
                params: e.params.iter().map(param).collect(),
                gates: e.gates.iter().map(gate).collect(),
            })
            .collect(),
        pinned: pe_effects::PINNED_ROWS.iter().map(|s| s.to_string()).collect(),
        visible_at_defaults: pe_effects::EFFECTS_WITH_VISIBLE_DEFAULTS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        groups: pe_effects::Group::ALL
            .iter()
            .map(|g| g.as_str().to_string())
            .collect(),
    }
}
```

Add to `crates/pe-session/src/lib.rs`:

```rust
pub mod describe;
```

`pe-color` is already a dependency of `pe-session` from Task 2's manifest, so
`pe_color::WorkingSpace` is nameable here without touching `Cargo.toml`.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 34 tests.

- [ ] **Step 5: Add the committed fixture, so Swift and Rust cannot drift apart**

Create `crates/pe-session/tests/fixtures.rs`:

```rust
//! The fixtures the Swift tests decode.
//!
//! Committed, and checked here against what the code produces now. If a field
//! is added in Rust and not in Swift, or renamed on one side, one of the two
//! suites fails — which is the only thing standing between two halves of one
//! application and a silent divergence.
//!
//! Regenerate deliberately, having looked at the diff:
//!
//! ```text
//! PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
//! ```

use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/apple/Fixtures")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/apple/Fixtures")
        })
}

fn check(name: &str, produced: String) {
    let path = fixture_dir().join(name);
    if std::env::var_os("PE_UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &produced).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("{} is missing — run with PE_UPDATE_FIXTURES=1", path.display())
    });
    assert_eq!(
        committed.trim(),
        produced.trim(),
        "{} is out of date. Look at the change, then regenerate with \
         PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures",
        path.display()
    );
}

#[test]
fn the_registry_fixture_is_current() {
    let json = serde_json::to_string_pretty(&pe_session::describe::registry()).unwrap();
    check("registry.json", json);
}
```

- [ ] **Step 6: Generate it and look at what it contains**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures && python3 -c "
import json; r = json.load(open('apps/apple/Fixtures/registry.json'))
print(len(r['effects']), 'effects,', sum(len(e['params']) for e in r['effects']), 'parameters')
print('pinned:', ', '.join(r['pinned']))"
```

Expected: the effect count matches the registry (30 at the time of writing) and
the pinned rows are listed. **Read a few entries.** This file is the contract
the Swift inspector is generated from; if a parameter looks wrong here it will
look wrong in the application.

- [ ] **Step 7: Run it again to prove the check works**

```bash
cargo test -p pe-session --test fixtures
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A crates/pe-session apps/apple/Fixtures
git commit -m "Thirty effects become a file Swift can read, so the inspector is eight views"
```

---

## Task 10: The snapshot Swift mirrors

The state a shell needs to draw itself, in one document. Derived, never
authored: mutations go one way in through typed calls, and this comes one way
back out.

**Files:**
- Modify: `crates/pe-session/src/describe.rs`, `crates/pe-session/src/session.rs`, `crates/pe-session/tests/fixtures.rs`
- Create: `apps/apple/Fixtures/snapshot.json`

- [ ] **Step 1: Write the failing tests**

Add to the test module in `describe.rs`:

```rust
    #[test]
    fn a_snapshot_of_nothing_is_still_a_snapshot() {
        // The shell draws an empty viewer from this rather than from a null.
        let s = crate::Session::new();
        let snap = snapshot(&s);
        assert!(!snap.is_open);
        assert!(snap.rows.is_empty());
        assert!(!snap.can_undo);
    }

    #[test]
    fn a_row_carries_everything_the_inspector_draws() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        let id = s.add_effect("exposure").unwrap();
        s.set_float(id, "ev", 0.75).unwrap();

        let snap = snapshot(&s);
        let row = snap.rows.iter().find(|r| r.id == id.0).unwrap();
        assert_eq!(row.effect, "exposure");
        assert!(row.enabled);
        assert_eq!(row.blend, "normal");
        assert!(!row.pinned);
        // Parameters keep the document's own representation, so there is one
        // shape on the wire and not two.
        let ev = row.params.get("ev").unwrap();
        assert_eq!(ev["t"], "float");
        assert_eq!(ev["v"], 0.75);
    }

    #[test]
    fn undo_shows_in_the_snapshot_and_says_what_it_would_undo() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        s.add_effect("exposure").unwrap();
        let snap = snapshot(&s);
        assert!(snap.can_undo);
        assert_eq!(snap.undo_label.as_deref(), Some("Add Exposure"));
    }

    #[test]
    fn the_version_moves_on_a_change_and_not_otherwise() {
        // What lets a shell skip decoding an unchanged frame for the cost of
        // one integer comparison.
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        let before = snapshot(&s).version;
        assert_eq!(snapshot(&s).version, before);
        s.add_effect("exposure").unwrap();
        assert!(snapshot(&s).version > before);
    }
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-session
```

Expected: FAIL to compile — `snapshot` unresolved.

- [ ] **Step 3: Write the implementation**

Add to `crates/pe-session/src/describe.rs`:

```rust
/// Everything a shell needs to draw itself, in one document.
///
/// Derived from the session, never authored. Mutations go one way in through
/// typed calls; this comes one way back out. Two directions and one source of
/// truth, which is what keeps a Swift `@Observable` idiomatic without becoming
/// a second implementation of the document model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Bumped by every mutation. Compare before decoding.
    pub version: u64,
    pub is_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// File name alone, for the title bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub rows: Vec<Row>,
    pub color: Color,
    /// Passes the last frame executed. The number that proves the stage cache
    /// works: with a nine-row stack, dragging the deepest slider reads 1.
    pub passes: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo_label: Option<String>,
    pub export_format: String,
    pub export_quality: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub id: u64,
    pub effect: String,
    pub enabled: bool,
    pub opacity: f32,
    pub blend: String,
    /// Fixed panels, which cannot be removed or reordered.
    pub pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The document's own parameter representation, verbatim — `{"t":"float",
    /// "v":0.35}`. One shape on the wire rather than two.
    pub params: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Color {
    pub input: String,
    pub output: String,
}

pub fn snapshot(session: &crate::Session) -> Snapshot {
    let doc = session.document();
    let (width, height) = session.image_size();
    let rows = doc
        .map(|d| {
            d.stack
                .iter()
                .map(|r| Row {
                    id: r.id.0,
                    effect: r.effect.clone(),
                    enabled: r.enabled,
                    opacity: r.opacity,
                    blend: serde_json::to_value(r.blend)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_else(|| "normal".into()),
                    pinned: r.pinned,
                    label: r.label.clone(),
                    params: r
                        .params
                        .0
                        .iter()
                        .filter_map(|(k, v)| {
                            Some((k.clone(), serde_json::to_value(v).ok()?))
                        })
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    let export = session.export_settings();
    Snapshot {
        version: session.snapshot_version(),
        is_open: session.is_open(),
        path: session.path().map(|p| p.display().to_string()),
        name: session
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string()),
        width,
        height,
        rows,
        color: Color {
            input: doc.map(|d| d.color.input.clone()).unwrap_or_default(),
            output: doc.map(|d| d.color.output.clone()).unwrap_or_default(),
        },
        passes: session.last_passes(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        undo_label: session.undo_label(),
        redo_label: session.redo_label(),
        export_format: export.format.name().to_string(),
        export_quality: export.quality,
    }
}
```

Add the three accessors `snapshot` needs to `impl Session` in `session.rs`:

```rust
    /// The source photograph's pixel dimensions, or zeroes when nothing is
    /// open. Zeroes rather than an `Option` because the shell divides by them
    /// to fit the view and wants one branch, not two.
    pub fn image_size(&self) -> (u32, u32) {
        self.photo
            .as_ref()
            .map_or((0, 0), |p| (p.image.width, p.image.height))
    }

    pub fn undo_label(&self) -> Option<String> {
        self.photo.as_ref()?.history.undo_label().map(str::to_string)
    }

    pub fn redo_label(&self) -> Option<String> {
        self.photo.as_ref()?.history.redo_label().map(str::to_string)
    }
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-session
```

Expected: PASS, 38 tests.

If `undo_shows_in_the_snapshot_and_says_what_it_would_undo` fails on the label,
print what it actually is — `add_effect` builds it as `format!("Add {}", def.name)`
and the registry's name for `exposure` decides the rest.

- [ ] **Step 5: Add the snapshot fixture**

Add to `crates/pe-session/tests/fixtures.rs`:

```rust
#[test]
fn the_snapshot_fixture_is_current() {
    // A session with something in it, so the fixture exercises rows,
    // parameters, undo labels and the colour settings rather than the empty
    // case. Deterministic: no clock, no GPU, no file.
    let mut s = pe_session::Session::new();
    s.open_test_chart(64, 64).unwrap();
    let id = s.add_effect("exposure").unwrap();
    s.set_float(id, "ev", 0.75).unwrap();
    let json = serde_json::to_string_pretty(&pe_session::describe::snapshot(&s)).unwrap();
    check("snapshot.json", json);
}
```

- [ ] **Step 6: Generate it and read it**

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures && cat apps/apple/Fixtures/snapshot.json
```

Expected: a document with `is_open: true`, the pinned rows plus the exposure
row, and `"ev": {"t": "float", "v": 0.75}`.

- [ ] **Step 7: Verify and commit**

```bash
cargo test -p pe-session && cargo clippy -p pe-session --all-targets -- -D warnings
```

```bash
git add -A crates/pe-session apps/apple/Fixtures
git commit -m "One document out, typed calls in: the shape a Swift view model mirrors"
```

---

## Task 11: The C ABI

Everything above, behind five rules. The first three already govern this file;
Tasks 4 and 5 of the spec add the last two.

**Files:**
- Modify: `crates/pe-ffi/src/lib.rs`, `crates/pe-ffi/Cargo.toml`
- Modify: `apps/apple/Spike/main.swift`

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/pe-ffi/src/lib.rs`:

```rust
    #[test]
    fn a_session_opens_a_chart_and_reports_its_rows() {
        let s = pe_session_new();
        assert!(!s.is_null());
        assert_eq!(unsafe { pe_session_open_test_chart(s, 64, 64) }, 0);
        assert!(unsafe { pe_session_row_count(s) } > 0);
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn every_entry_point_survives_a_null_handle() {
        // A null here is a Swift bug, and a crash inside Rust tells nobody
        // anything useful about where it was. Each of these returns its
        // failure value instead.
        assert_eq!(unsafe { pe_session_row_count(ptr::null_mut()) }, -1);
        assert_eq!(unsafe { pe_session_open_test_chart(ptr::null_mut(), 8, 8) }, -1);
        assert!(unsafe { pe_session_snapshot_json(ptr::null_mut()) }.is_null());
        assert_eq!(unsafe { pe_session_snapshot_version(ptr::null_mut()) }, 0);
        assert_eq!(unsafe { pe_session_undo(ptr::null_mut()) }, -1);
        unsafe { pe_session_free(ptr::null_mut()) };
    }

    #[test]
    fn a_parameter_the_effect_does_not_have_is_refused_with_a_message() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let key = cstr("exposure");
        let row = unsafe { pe_session_add_effect(s, key.as_ptr()) };
        assert!(row >= 0);

        let bad = cstr("not_a_parameter");
        assert_ne!(unsafe { pe_session_set_float(s, row as u64, bad.as_ptr(), 1.0) }, 0);

        let msg = unsafe { pe_session_last_error(s) };
        assert!(!msg.is_null(), "a failure with no message is a failure nobody can report");
        let text = unsafe { CStr::from_ptr(msg) }.to_str().unwrap().to_owned();
        assert!(text.contains("not_a_parameter"), "unhelpful message: {text}");
        unsafe { pe_string_free(msg) };
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_drag_bracketed_by_an_interaction_is_one_undo_step() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let key = cstr("exposure");
        let row = unsafe { pe_session_add_effect(s, key.as_ptr()) } as u64;
        let ev = cstr("ev");
        let label = cstr("Exposure");

        unsafe { pe_session_begin_interaction(s, label.as_ptr()) };
        for i in 1..60 {
            unsafe { pe_session_set_float(s, row, ev.as_ptr(), i as f32 * 0.01) };
        }
        unsafe { pe_session_end_interaction(s) };

        // One undo puts the whole drag back — not one frame of it. Fifty-nine
        // undo steps would mean the coalescing bracket did nothing.
        assert_eq!(unsafe { pe_session_undo(s) }, 1);

        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ev = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .and_then(|r| r["params"]["ev"]["v"].as_f64())
            .expect("the exposure row is still there");
        assert_eq!(ev, 0.0, "one undo left the drag partly applied: ev is {ev}");

        // And a second undo takes the row away, so the drag really was one step.
        assert_eq!(unsafe { pe_session_undo(s) }, 1);
        assert!(!unsafe { pe_session_can_undo(s) });
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn the_registry_crosses_the_boundary_whole() {
        let json = pe_registry_json();
        assert!(!json.is_null());
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed["effects"].as_array().unwrap().len(),
            pe_effects::all().len()
        );
    }
```

Add `serde_json = "1"` and `pe-effects = { version = "0.0.1", path = "../pe-effects" }` to `crates/pe-ffi/Cargo.toml` under `[dev-dependencies]` and `[dependencies]` respectively.

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p pe-ffi
```

Expected: FAIL to compile — none of the `pe_session_*` functions exist.

- [ ] **Step 3: Write the implementation**

Append to `crates/pe-ffi/src/lib.rs`. Delete `pe_spike_attach_and_clear` from
Task 3 first — `pe_session_attach_layer` replaces it.

```rust
// ---------------------------------------------------------------------------
// The session.
//
// Two rules on top of the three above.
//
// 4. Hot paths are typed scalars; cold paths are JSON. A slider drag must not
//    allocate a string. Structure is rare and shape-heavy, so it goes as JSON
//    where adding a field does not mean adding a function.
//
// 5. Nothing calls back into Swift. Swift drives and Rust answers. A callback
//    from a Rust worker thread into a Swift closure is a reentrancy bug with a
//    deadline on it.
// ---------------------------------------------------------------------------

use pe_session::Session;

/// Opaque handle to a session, plus the last thing that went wrong.
///
/// The message is kept here rather than returned, because every fallible entry
/// point returns `i32` and a status code with no text is a bug report nobody
/// can write.
pub struct PeSession {
    inner: Session,
    last_error: Option<String>,
}

/// Run `f` against a session, or return `fallback` for a null handle.
fn with<T>(s: *mut PeSession, fallback: T, f: impl FnOnce(&mut PeSession) -> T) -> T {
    guard(fallback, || match unsafe { s.as_mut() } {
        Some(s) => f(s),
        None => fallback,
    })
}

/// The same, for a call that returns a status code and may set an error.
fn status(
    s: *mut PeSession,
    f: impl FnOnce(&mut Session) -> Result<(), pe_session::SessionError>,
) -> i32 {
    with(s, -1, |s| match f(&mut s.inner) {
        Ok(()) => {
            s.last_error = None;
            0
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

fn as_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

fn to_c(s: String) -> *mut c_char {
    CString::new(s).map(|c| c.into_raw()).unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn pe_session_new() -> *mut PeSession {
    guard(ptr::null_mut(), || {
        Box::into_raw(Box::new(PeSession {
            inner: Session::new(),
            last_error: None,
        }))
    })
}

/// # Safety
/// `s` must have come from [`pe_session_new`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_free(s: *mut PeSession) {
    if !s.is_null() {
        drop(unsafe { Box::from_raw(s) });
    }
}

/// The last failure's message, or null if the last call succeeded.
/// Caller must release with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_last_error(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.last_error.clone() {
        Some(m) => to_c(m),
        None => ptr::null_mut(),
    })
}

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_support_dir(
    s: *mut PeSession,
    path: *const c_char,
) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| {
        s.set_support_dir(path);
        Ok(())
    })
}

// ---- opening --------------------------------------------------------------

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_open_path(s: *mut PeSession, path: *const c_char) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| s.open_path(path))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_open_test_chart(
    s: *mut PeSession,
    width: u32,
    height: u32,
) -> i32 {
    status(s, move |s| s.open_test_chart(width, height))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_row_count(s: *mut PeSession) -> i64 {
    with(s, -1, |s| s.inner.row_count() as i64)
}

// ---- the screen -----------------------------------------------------------

/// Adopt a `CAMetalLayer`.
///
/// # Safety
/// `layer` must be a live `CAMetalLayer` that outlives the attachment, and
/// [`pe_session_detach_layer`] must be called before it goes away.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_attach_layer(
    s: *mut PeSession,
    layer: *mut std::ffi::c_void,
    width: u32,
    height: u32,
) -> i32 {
    status(s, move |s| unsafe { s.attach_layer(layer, width, height) })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_resize(s: *mut PeSession, width: u32, height: u32) -> i32 {
    status(s, move |s| {
        s.resize_layer(width, height);
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_detach_layer(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.detach_layer();
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_render(s: *mut PeSession) -> i32 {
    status(s, |s| s.present())
}

/// Whether anything has changed since the last present. The display link asks
/// this before doing any work; an editor that redraws 120 times a second while
/// nothing moves is a laptop with a warm keyboard.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_needs_render(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.needs_render())
}

/// Passes the last frame executed. See `Snapshot::passes`.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_last_passes(s: *mut PeSession) -> i64 {
    with(s, -1, |s| s.inner.last_passes() as i64)
}

/// Drive the autosave debounce. Called from the display link.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_tick(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.tick();
        Ok(())
    })
}

// ---- the document ---------------------------------------------------------

/// The whole UI-visible state. Caller must release with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_snapshot_json(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| {
        match serde_json::to_string(&pe_session::describe::snapshot(&s.inner)) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}

/// Bumped by every mutation. Compare before decoding the snapshot.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_snapshot_version(s: *mut PeSession) -> u64 {
    with(s, 0, |s| s.inner.snapshot_version())
}

/// Returns the new row's id, or negative on failure.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_add_effect(s: *mut PeSession, key: *const c_char) -> i64 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    with(s, -1, |s| match s.inner.add_effect(&key) {
        Ok(id) => {
            s.last_error = None;
            id.0 as i64
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_remove_row(s: *mut PeSession, row: u64) -> i32 {
    status(s, move |s| s.remove_row(pe_core::RowId(row)))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_move_row(s: *mut PeSession, row: u64, to: u32) -> i32 {
    status(s, move |s| s.move_row(pe_core::RowId(row), to as usize))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_row_enabled(
    s: *mut PeSession,
    row: u64,
    on: bool,
) -> i32 {
    status(s, move |s| s.set_row_enabled(pe_core::RowId(row), on))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_row_opacity(
    s: *mut PeSession,
    row: u64,
    value: f32,
) -> i32 {
    status(s, move |s| s.set_row_opacity(pe_core::RowId(row), value))
}

// ---- parameters, the hot path ---------------------------------------------

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_float(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_float(pe_core::RowId(row), &key, value))
}

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_bool(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: bool,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_bool(pe_core::RowId(row), &key, value))
}

/// # Safety
/// `s`, `key` and `value` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_choice(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: *const c_char,
) -> i32 {
    let (Some(key), Some(value)) = (as_str(key), as_str(value)) else {
        return -1;
    };
    let (key, value) = (key.to_string(), value.to_string());
    status(s, move |s| s.set_choice(pe_core::RowId(row), &key, &value))
}

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_rgb(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    r: f32,
    g: f32,
    b: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_rgb(pe_core::RowId(row), &key, [r, g, b]))
}

// ---- history --------------------------------------------------------------

/// Bracket a drag so it becomes one undo step rather than three hundred.
///
/// # Safety
/// `s` and `label` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_begin_interaction(
    s: *mut PeSession,
    label: *const c_char,
) -> i32 {
    let label = as_str(label).unwrap_or("Edit").to_string();
    status(s, move |s| {
        s.begin_interaction(label);
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_end_interaction(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.end_interaction();
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_can_undo(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.can_undo())
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_can_redo(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.can_redo())
}

/// `1` if it moved, `0` if there was nothing to undo, negative on failure.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_undo(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.undo() {
        Ok(moved) => moved as i32,
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_redo(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.redo() {
        Ok(moved) => moved as i32,
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

// ---- persistence and export -----------------------------------------------

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_save_sidecar(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.inner.save_sidecar() {
        Ok(p) => {
            s.last_error = None;
            to_c(p.display().to_string())
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            ptr::null_mut()
        }
    })
}

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_load_sidecar(
    s: *mut PeSession,
    path: *const c_char,
) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| s.load_sidecar(path))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_revert(s: *mut PeSession) -> i32 {
    status(s, |s| s.revert())
}

/// # Safety
/// `s` and `format` must be valid or null. `format` is one of `jpeg`, `png`,
/// `png16`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_export(
    s: *mut PeSession,
    format: *const c_char,
    quality: u8,
) -> i32 {
    let name = as_str(format).unwrap_or("jpeg").to_string();
    status(s, move |s| {
        s.set_export(pe_session::export::Format::from_name(&name), quality);
        Ok(())
    })
}

/// Export, returning the path written. Null on failure; the reason is in
/// [`pe_session_last_error`], and "refused" there is not a bug.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_export(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.inner.export_current() {
        Ok(p) => {
            s.last_error = None;
            to_c(p.display().to_string())
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            ptr::null_mut()
        }
    })
}

// ---- the registry ---------------------------------------------------------

/// Every effect and every parameter, as JSON. Called once at launch; the whole
/// inspector is generated from it. Caller must release with [`pe_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn pe_registry_json() -> *mut c_char {
    guard(ptr::null_mut(), || {
        match serde_json::to_string(&pe_session::describe::registry()) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}
```

Add to `crates/pe-ffi/Cargo.toml` under `[dependencies]`:

```toml
pe-session = { version = "0.0.1", path = "../pe-session" }
pe-effects = { version = "0.0.1", path = "../pe-effects" }
serde_json = "1"
```

and remove `wgpu`, `pollster` and `pe-render` if Task 3 added them and nothing
else uses them — `cargo clippy` will say so via `unused_crate_dependencies` only
if that lint is on, so check by removing them and rebuilding.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p pe-ffi
```

Expected: PASS. The five original tests plus the five new ones.

- [ ] **Step 5: Repoint the spike at the real API**

In `apps/apple/Spike/main.swift`, replace the `pe_spike_attach_and_clear` call
with the session it became:

```swift
        let session = pe_session_new()
        let rc = pe_session_attach_layer(
            session,
            ptr,
            UInt32(metal.drawableSize.width),
            UInt32(metal.drawableSize.height)
        )
        if rc == 0 {
            pe_session_open_test_chart(session, 512, 512)
            pe_session_render(session)
        }
        attached = true
```

- [ ] **Step 6: Build and run the spike again**

```bash
cd apps/apple && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme Spike -configuration Debug build
```

```bash
cd apps/apple && open "$(xcodebuild -project PhotoEditor.xcodeproj -scheme Spike -configuration Debug -showBuildSettings | awk -F' = ' '/ BUILT_PRODUCTS_DIR/ {print $2}')/Spike.app"
```

Expected: **the built-in test chart, rendered through the full pipeline, in a
Swift-owned window.** That is the whole port proven end to end — decode,
working space, stack, display transform, Metal layer.

Compare it against the same chart on the Windows build (`cargo run -p
pe-windows --release`). They should look the same. If the Mac one is noticeably
dark or light, the surface format is applying the transfer function a second
time — see the note in `preview.rs:439` about `raw_view`, which is the same
trap.

- [ ] **Step 7: Commit**

```bash
git add -A crates/pe-ffi apps/apple Cargo.lock
git commit -m "Fifty functions, five rules, and a test chart in a window Swift made"
```

---

## Task 12: CI, and the documents that describe the layout

**Files:**
- Modify: `.github/workflows/ci.yml`, `README.md`, `apps/apple/README.md`

- [ ] **Step 1: Teach CI about the Apple side**

In `.github/workflows/ci.yml`, add this job after `ffi-header`:

```yaml
  apple:
    name: the Apple shells build
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-darwin, x86_64-apple-darwin
      - uses: Swatinem/rust-cache@v2
      - name: Install tools
        run: brew install xcodegen && cargo install cbindgen --locked
      # The fixtures the Swift tests decode are committed. If a field is added
      # in Rust and not regenerated, this is where it is noticed — before the
      # Swift side fails to decode something it has never seen.
      - name: Fixtures are current
        run: cargo test -p pe-session --test fixtures
      - name: Build the spike
        run: |
          cd apps/apple
          xcodegen generate
          xcodebuild -project PhotoEditor.xcodeproj -scheme Spike \
                     -configuration Debug build CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 2: Update the layout section of the README**

In `README.md`, replace the `crates/` and `apps/` block under **Layout** with:

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
  apple/       macOS and iOS shells, and the Swift package they share
tests/golden/  reference renders, diffed by CI
docs/          the two decision records worth reading first
```

And extend **The firewall rule** with the paragraph that Tasks 4 and 5 made
true:

```markdown
The rule extends downward as well. `pe-session` holds the things that are
neither pixels nor interface — which photograph is open, where work in progress
is kept, and what may be written where. "Never write over an original" is not a
Windows rule, and a rule implemented twice is a rule that will differ.
```

- [ ] **Step 3: Rewrite the Apple README**

Replace `apps/apple/README.md` with:

```markdown
# The Apple shells

macOS and iOS/iPadOS, over the same engine Windows uses.

## Building

```bash
brew install xcodegen
cargo install cbindgen
cd apps/apple && xcodegen generate && open PhotoEditor.xcodeproj
```

`build-engine.sh` runs as a pre-build phase: it compiles `pe-ffi` for both
Apple architectures, `lipo`s them into a universal static library, and
regenerates `pe_ffi.h`.

## Targets

| | |
|---|---|
| `Spike` | The smallest thing that proves the layer path: a `CAMetalLayer` made in Swift, filled by wgpu in Rust. Kept because it is the fastest way to tell whether a graphics problem is in the engine or in the shell. |
| `PhotoEditor` | The macOS application. |

## Why the .xcodeproj is generated

A `project.pbxproj` is unmergeable — every branch that adds a file conflicts.
`project.yml` is the source of truth; the Xcode project is a build artefact.

## Fixtures

`Fixtures/` holds `registry.json` and `snapshot.json`, written by
`cargo test -p pe-session --test fixtures` and decoded by the Swift tests. They
are how the two halves of one application are stopped from drifting apart: add
a field in Rust without adding it in Swift and one of the two suites fails.

Regenerate deliberately, having looked at the diff:

```bash
PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
```

## What is deliberately absent

No image processing, no colour maths, no shaders — and no workflow rules
either. Where a file may be written, what an export is called and when work in
progress is saved all live in `crates/pe-session`, where Windows uses the same
code and the tests cover it once.
```

- [ ] **Step 4: Full verification**

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Expected: no output from the first two; all tests pass.

```bash
cd apps/apple && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme Spike -configuration Debug build CODE_SIGNING_ALLOWED=NO
```

Expected: `BUILD SUCCEEDED`.

- [ ] **Step 5: Commit**

```bash
git add -A .github README.md apps/apple/README.md
git commit -m "CI builds the Mac side, and the layout section stops describing an older repository"
```

---

## Done when

- [ ] `cargo test --workspace` passes on macOS, including the golden tests.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is silent.
- [ ] The `Spike` target shows the built-in test chart, rendered by the engine, in a Swift-owned window — and it matches what `cargo run -p pe-windows --release` shows.
- [ ] `apps/apple/Fixtures/registry.json` describes every registered effect, and its test fails if the registry changes without it.
- [ ] `apps/windows` builds and behaves as before, using `pe_session::autosave` and `pe_session::export` rather than its own copies.
- [ ] Nothing in `apps/windows` decides where the support directory is except `platform_support()`.

## What this plan deliberately does not do

- **`KromaKit` and the macOS shell.** The second plan. Its Swift depends on the generated `pe_ffi.h` and on the surface format Task 11 Step 6 settles.
- **The library, thumbnails and settings extraction.** `apps/windows/src/library.rs` and `settings.rs` stay where they are until the Mac shell needs an open set, which is a filmstrip and therefore breadth, not the vertical slice.
- **Converting `apps/windows` onto `Session`.** Its `App` is entangled with egui's frame loop by design. The shared rules moved; the aggregate is new code for the Swift shells.
- **Four groups from the spec's FFI table.** Named here so their absence is a decision rather than an oversight:
  - `pe_session_set_view(zoom, pan_x, pan_y)` — inseparable from the viewer interaction it serves (scroll-to-zoom anchored under the cursor), so it lands with the viewer in plan two. Until then every render is `Region::FULL`.
  - `pe_session_set_geometry_json` — crop and straighten are breadth, not the slice.
  - `pe_session_set_color_settings` — belongs with the File page.
  - `pe_session_export_all` / `_export_progress` / `_poll_events` — nothing runs on a worker thread yet, so there is nothing to poll. They arrive with the open set and batch export.
- **The extended-range display path.** `CAMetalLayer.colorspace` and `wantsExtendedDynamicRangeContent` are CoreAnimation properties set from Swift, so they belong to plan two. Task 11 Step 6 settles the ordinary sRGB case first, which is what that work would build on.
- **iOS.** No `Source` variant for Photos assets, no touch, no `x86_64` simulator target. Its own spec.
