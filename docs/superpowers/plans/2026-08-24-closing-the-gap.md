# Closing the Gap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Take the macOS application from a vertical slice to something a person can actually grade a photograph with — every effect reachable, four more control kinds drawn, and a viewer you can zoom.

**Architecture:** Unchanged. Rust owns the document; Swift mutates through typed C calls and mirrors an immutable snapshot. Every control is still generated from the effect registry rather than written per effect, so each new `ParamKind` case unlocks it everywhere at once.

**Tech Stack:** Swift 6 / SwiftUI / AppKit, XcodeGen, XCTest, the `pe-ffi` C ABI (44 functions, two added here).

**Predecessors:** `2026-08-23-apple-foundation.md` and `2026-08-24-kromakit-macos-slice.md`, both complete and merged to `master`.

---

## Where the two shells stand

| | Windows (egui) | macOS (native) |
|---|---|---|
| lines | 13,100 across 16 modules | 1,325 |
| effects reachable | all 30 | **11** — only the pinned rows |
| parameter kinds drawn | 8 of 8 | **1 of 8** (float) |
| viewer | zoom, pan, fit, compare | fits the frame only |

Seven of the eleven pinned panels are already complete, because their parameters
are all floats — including Colour Mixer, which has twenty-four of them. What is
missing splits cleanly:

| to finish | parameters |
|---|---|
| `wheel` | 8 — completes Lift/Gamma/Gain and Log Wheels |
| `choice` | 27 across the registry, 4 of them in Colour Warper |
| `bool` | 36 |
| `rgb` | 11 |
| `curve` | 10 — Custom Curves, its own plan |
| `warp` + `pins` | 4 — the Colour Warper's lattices, its own plan |

**The larger gap is not a control kind.** Nineteen of the thirty effects cannot be
reached at all, because nothing in the Mac app adds a row to the stack. That is
Task 6, and it is worth more than any single control.

## Scope

This plan does bool, choice, rgb and wheel; the effect browser and stack
management; and zoom and pan. It does **not** do the curve editor or the colour
warper — each is a substantial custom-drawn view (1,693 and 1,021 lines
respectively in the egui shell) and each deserves its own plan. Nor scopes,
crop, filmstrip, batch export or compare.

At the end of it: 317 of 339 parameters drawn, every effect reachable, and a
viewer you can work in.

---

## File Structure

**Created:**

| path | responsibility |
|---|---|
| `apps/apple/KromaKit/Controls/BoolRow.swift` | a checkbox in the four-column row |
| `apps/apple/KromaKit/Controls/ChoiceRow.swift` | a dropdown in the four-column row |
| `apps/apple/KromaKit/Controls/RgbRow.swift` | a colour well in the four-column row |
| `apps/apple/KromaKit/Controls/WheelView.swift` | Resolve's four-way colour wheel |
| `apps/apple/KromaKit/Controls/WheelGeometry.swift` | wheel arithmetic, testable without a view |
| `apps/apple/KromaKit/EffectBrowser.swift` | the menu that adds a row |
| `apps/apple/KromaKit/StackRowView.swift` | one non-pinned row: name, enable, opacity, blend, remove, reorder |
| `apps/apple/KromaKit/ViewState.swift` | zoom and pan arithmetic, testable without a view |

**Moved:** `ParameterRow.swift` gains nothing; the existing `FloatRow` moves into
`Controls/` alongside its siblings so the eight kinds sit together.

**Modified:** `Snapshot.swift` (a `.wheel` case), `Engine.swift` and
`SessionStore.swift` (wheel and view calls), `InspectorPanel.swift` (four more
switch arms), `ContentView.swift`, `crates/pe-ffi/src/lib.rs` (two functions),
`crates/pe-session/src/session.rs` (view state).

---

## Task 1: A wheel is a value Swift can read

`ParamValue` decodes `wheel` as `.opaque`, so a wheel control could be drawn but
never shown holding its current value. The wire shape is
`{"t":"wheel","v":{"master":1.0,"rgb":[1.0,1.0,1.0]}}`.

**Files:**
- Modify: `apps/apple/KromaKit/Snapshot.swift`
- Modify: `apps/apple/KromaKitTests/SnapshotTests.swift`

- [ ] **Step 1: Write the failing test**

Add to `SnapshotTests`:

```swift
    func testAWheelDecodesItsFourComponents() throws {
        // Resolve's wheels are four-valued: three channels and the luminance
        // ring around the outside. The master is modelled separately rather
        // than folded into the channels, so that resetting just the ring stays
        // possible — the same reason pe-core keeps them apart.
        let json = Data(#"{"k":{"t":"wheel","v":{"master":1.0,"rgb":[0.25,0.5,0.75]}}}"#.utf8)
        let values = try JSONDecoder().decode([String: ParamValue].self, from: json)
        guard case let .wheel(w) = try XCTUnwrap(values["k"]) else {
            return XCTFail("not a wheel")
        }
        XCTAssertEqual(w.master, 1.0, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[0], 0.25, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[1], 0.5, accuracy: 0.0001)
        XCTAssertEqual(w.rgb[2], 0.75, accuracy: 0.0001)
    }

    func testTheCommittedSnapshotCarriesReadableWheels() throws {
        // primaries and log_wheels are pinned, so every fresh document has
        // wheels in it. If they decode as opaque the panels cannot draw.
        let snap = try JSONDecoder().decode(Snapshot.self, from: fixture("snapshot"))
        let primaries = try XCTUnwrap(snap.rows.first { $0.effect == "primaries" })
        guard case let .wheel(gain) = try XCTUnwrap(primaries.params["gain"]) else {
            return XCTFail("gain is not a wheel")
        }
        XCTAssertEqual(gain.master, 1.0, accuracy: 0.0001)
    }
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | tail -20
```

Expected: compile failure, `type 'ParamValue' has no member 'wheel'`.

- [ ] **Step 3: Write the implementation**

In `apps/apple/KromaKit/Snapshot.swift`, add the case and its payload:

```swift
/// A four-way colour wheel's value.
///
/// Three channels and a master. `pe-core` keeps the master separate rather than
/// folding it into the channels so that resetting only the outer ring stays
/// possible, and the wire shape follows.
public struct WheelValue: Decodable, Sendable, Equatable {
    public let rgb: [Float]
    public let master: Float

    public init(rgb: [Float], master: Float) {
        self.rgb = rgb
        self.master = master
    }
}
```

Add `case wheel(WheelValue)` to `ParamValue`, and in `init(from:)` replace the
`default` arm's handling of `"wheel"` by adding this case before it:

```swift
        case "wheel":
            self = .wheel(try c.decode(WheelValue.self, forKey: .v))
```

Add an accessor beside `floatValue`:

```swift
    /// The value as a wheel, for the control that draws one.
    public var wheelValue: WheelValue? {
        if case let .wheel(w) = self { return w }
        return nil
    }
```

- [ ] **Step 4: Run the tests**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | LC_ALL=C grep -aE "error:|\*\* TEST|Executed [0-9]+ test"
```

Expected: `** TEST SUCCEEDED **`, 30 tests.

- [ ] **Step 5: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "A wheel arrives as four numbers rather than as something unreadable"
```

---

## Task 2: The three simple controls

Bool, choice and rgb between them are 74 of the registry's 339 parameters. With
float they cover 317 — ninety-four per cent of everything the application can
be asked to draw.

**Files:**
- Create: `apps/apple/KromaKit/Controls/BoolRow.swift`
- Create: `apps/apple/KromaKit/Controls/ChoiceRow.swift`
- Create: `apps/apple/KromaKit/Controls/RgbRow.swift`
- Modify: `apps/apple/KromaKit/InspectorPanel.swift`
- Modify: `apps/apple/KromaKit/SessionStore.swift`

**`SessionStore` is missing a `setRGB` wrapper.** `Session` has one in
`Engine.swift`, but the store never got the parallel method — an oversight from
the plan that built it, invisible until a control needed to call it. Add it
beside `setChoice`, in the same shape as its neighbours:

```swift
    public func setRGB(row: UInt64, key: String, _ r: Float, _ g: Float, _ b: Float) {
        run { try session.setRGB(row: row, key: key, r, g, b) }
        refresh()
    }
```

There is no unit test for these three: they are each a stock SwiftUI control in
the four-column row, with no arithmetic of their own. The row metrics they use
are already covered, and the switch that selects them is exercised by the
application building and running. Do not invent tests that assert a `Toggle` is
a `Toggle`.

- [ ] **Step 1: Write the checkbox**

Create `apps/apple/KromaKit/Controls/BoolRow.swift`:

```swift
import SwiftUI

/// A switch, in the same four-column row as everything else.
///
/// Its own file rather than another arm of a growing switch statement, because
/// there will be eight of these and a single file holding all of them is the
/// thing `inspector.rs` avoided on the Windows side.
public struct BoolRow: View {
    let param: Param
    let row: UInt64
    let value: Bool
    let isActive: Bool
    let store: SessionStore

    public init(param: Param, row: UInt64, value: Bool, isActive: Bool, store: SessionStore) {
        self.param = param
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    public var body: some View {
        HStack(spacing: RowMetrics.gap) {
            Text(param.name)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)
                .foregroundStyle(isActive ? .primary : .tertiary)

            Toggle("", isOn: Binding(
                get: { value },
                set: { store.setBool(row: row, key: param.key, value: $0) }
            ))
            .labelsHidden()
            .toggleStyle(.checkbox)

            Spacer()
        }
        .frame(height: RowMetrics.height)
        .disabled(!isActive)
    }
}
```

- [ ] **Step 2: Write the dropdown**

Create `apps/apple/KromaKit/Controls/ChoiceRow.swift`:

```swift
import SwiftUI

/// One of an effect's enumerated options.
///
/// The options come from the registry, so a choice added in Rust appears here
/// with nothing written on this side.
public struct ChoiceRow: View {
    let param: Param
    let options: [String]
    let row: UInt64
    let value: String
    let isActive: Bool
    let store: SessionStore

    public init(
        param: Param, options: [String], row: UInt64, value: String,
        isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.options = options
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    public var body: some View {
        HStack(spacing: RowMetrics.gap) {
            Text(param.name)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)
                .foregroundStyle(isActive ? .primary : .tertiary)

            Picker("", selection: Binding(
                get: { value },
                set: { store.setChoice(row: row, key: param.key, value: $0) }
            )) {
                ForEach(options, id: \.self) { option in
                    Text(option).tag(option)
                }
            }
            .labelsHidden()
            .frame(maxWidth: 140)

            Spacer()
        }
        .frame(height: RowMetrics.height)
        .disabled(!isActive)
    }
}
```

- [ ] **Step 3: Write the colour well**

Create `apps/apple/KromaKit/Controls/RgbRow.swift`:

```swift
import SwiftUI

/// A colour, in the working gamut.
///
/// Resolve exposes these with an eyedropper — Haze Color, Dirt Color, Scratch
/// Color. The eyedropper needs to sample the rendered frame, which means asking
/// the engine what colour a pixel is, and there is no call for that yet. The
/// well alone is most of the value.
public struct RgbRow: View {
    let param: Param
    let row: UInt64
    let value: [Float]
    let isActive: Bool
    let store: SessionStore

    public init(param: Param, row: UInt64, value: [Float], isActive: Bool, store: SessionStore) {
        self.param = param
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private var colour: Color {
        // The value is linear working-gamut RGB; SwiftUI wants something to
        // put on screen. `.sRGBLinear` is the honest reading of it, and lets
        // the well show roughly what the effect will do rather than a colour
        // that has been gamma-encoded twice.
        Color(
            .sRGBLinear,
            red: Double(value.first ?? 0),
            green: Double(value.dropFirst().first ?? 0),
            blue: Double(value.dropFirst(2).first ?? 0)
        )
    }

    public var body: some View {
        HStack(spacing: RowMetrics.gap) {
            Text(param.name)
                .frame(width: RowMetrics.label, alignment: .trailing)
                .lineLimit(1)
                .foregroundStyle(isActive ? .primary : .tertiary)

            // `Binding<Color>` explicitly: `ColorPicker` has both a
            // `Binding<Color>` and a `Binding<CGColor>` initialiser on macOS,
            // and an unannotated `Binding(get:set:)` infers the wrong one and
            // then fails to type-check the getter against it.
            ColorPicker("", selection: Binding<Color>(
                get: { colour },
                set: { picked in
                    let c = NSColor(picked).usingColorSpace(.extendedSRGB) ?? .black
                    // Back to linear, because that is what the effect works in
                    // and what the document stores.
                    let f = { (v: CGFloat) -> Float in
                        let s = Double(v)
                        return Float(s <= 0.04045 ? s / 12.92 : pow((s + 0.055) / 1.055, 2.4))
                    }
                    store.setRGB(
                        row: row, key: param.key,
                        f(c.redComponent), f(c.greenComponent), f(c.blueComponent)
                    )
                }
            ), supportsOpacity: false)
            .labelsHidden()

            Spacer()
        }
        .frame(height: RowMetrics.height)
        .disabled(!isActive)
    }
}
```

Add `import AppKit` at the top for `NSColor`.

- [ ] **Step 4: Route them from the panel**

In `apps/apple/KromaKit/InspectorPanel.swift`, add three arms to the switch in
`control(for:)`, before the `default:`:

```swift
        case let .bool(defaultValue):
            BoolRow(
                param: param,
                row: row.id,
                value: {
                    if case let .bool(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )

        case let .choice(options, defaultValue):
            ChoiceRow(
                param: param,
                options: options,
                row: row.id,
                value: {
                    if case let .choice(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )

        case let .rgb(defaultValue):
            RgbRow(
                param: param,
                row: row.id,
                value: {
                    if case let .rgb(v) = row.params[param.key] { return v }
                    return defaultValue
                }(),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )
```

- [ ] **Step 5: Build and run**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme PhotoEditor -configuration Debug build CODE_SIGNING_ALLOWED=NO 2>&1 | tail -3
```

Expected: `** BUILD SUCCEEDED **`. Then run it as the previous plan does, with
`NSUnbufferedIO=YES`, and confirm it survives six seconds printing nothing.

- [ ] **Step 6: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "Three more control kinds, and ninety-four per cent of the registry draws"
```

---

## Task 3: The wheel's arithmetic, without a wheel

Two pinned panels — Lift/Gamma/Gain and Log Wheels — are nothing but wheels. The
part worth testing is the mapping between a point in a circle and three channel
values, which a view makes untestable.

**Files:**
- Create: `apps/apple/KromaKit/Controls/WheelGeometry.swift`
- Create: `apps/apple/KromaKitTests/WheelGeometryTests.swift`

- [ ] **Step 1: Write the failing test**

Create `apps/apple/KromaKitTests/WheelGeometryTests.swift`:

```swift
import XCTest
// Same module as the code under test; see EngineTests.swift.

final class WheelGeometryTests: XCTestCase {
    private let bounds = Bounds(min: -1, max: 1, default: 0, neutral: 0)

    func testTheCentreIsNoChange() {
        // A wheel at rest sits at its neutral in every channel, and the handle
        // sits in the middle. Resolve's Gain rests at one and Offset at
        // twenty-five, so "no change" is not "zero" and the centre is not the
        // bottom of the range.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let rgb = g.rgb(at: .zero)
        XCTAssertEqual(rgb[0], 0, accuracy: 0.0001)
        XCTAssertEqual(rgb[1], 0, accuracy: 0.0001)
        XCTAssertEqual(rgb[2], 0, accuracy: 0.0001)
    }

    func testAGainWheelRestsAtOneInTheCentre() {
        let gain = Bounds(min: 0.01, max: 16, default: 1, neutral: 1)
        let g = WheelGeometry(bounds: gain, radius: 50)
        for c in g.rgb(at: .zero) {
            XCTAssertEqual(c, 1, accuracy: 0.0001)
        }
    }

    func testPullingTowardsRedRaisesRedAndLowersTheOthers() {
        // The three channels sit at 90, 210 and 330 degrees. Dragging towards
        // one of them has to raise it and lower the other two, or the wheel is
        // a brightness control with extra steps.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let towardsRed = g.point(forAngle: WheelGeometry.redAngle, distance: 25)
        let rgb = g.rgb(at: towardsRed)
        XCTAssertGreaterThan(rgb[0], 0)
        XCTAssertLessThan(rgb[1], 0)
        XCTAssertLessThan(rgb[2], 0)
    }

    func testTheThreeChannelsSumToNothingSoTheWheelDoesNotShiftBrightness() {
        // A colour wheel moves hue and leaves level alone; the ring beside it
        // is what moves level. If the three did not cancel, every hue push
        // would also be an exposure push.
        let g = WheelGeometry(bounds: bounds, radius: 50)
        for angle in stride(from: 0.0, to: 360.0, by: 15.0) {
            let rgb = g.rgb(at: g.point(forAngle: angle, distance: 30))
            XCTAssertEqual(rgb[0] + rgb[1] + rgb[2], 0, accuracy: 0.0001,
                           "a push at \(angle) degrees changed the level")
        }
    }

    func testADragOutsideTheCircleStopsAtTheEdge() {
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let far = g.rgb(at: CGPoint(x: 500, y: 0))
        let edge = g.rgb(at: CGPoint(x: 50, y: 0))
        for c in 0..<3 {
            XCTAssertEqual(far[c], edge[c], accuracy: 0.0001)
        }
    }

    func testAZeroRadiusWheelDoesNotDivideByZero() {
        // A view is laid out at zero size for at least one pass, and a NaN
        // that reaches a Shape is a control that never draws again.
        let g = WheelGeometry(bounds: bounds, radius: 0)
        for c in g.rgb(at: CGPoint(x: 10, y: 10)) {
            XCTAssertFalse(c.isNaN)
        }
        XCTAssertFalse(g.point(forAngle: 90, distance: 10).x.isNaN)
    }

    func testAValueRoundTripsBackToAPoint() {
        let g = WheelGeometry(bounds: bounds, radius: 50)
        let start = g.point(forAngle: 210, distance: 20)
        let back = g.point(for: g.rgb(at: start))
        XCTAssertEqual(back.x, start.x, accuracy: 0.5)
        XCTAssertEqual(back.y, start.y, accuracy: 0.5)
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | tail -15
```

Expected: compile failure, `cannot find 'WheelGeometry' in scope`.

- [ ] **Step 3: Write the implementation**

Create `apps/apple/KromaKit/Controls/WheelGeometry.swift`:

```swift
import CoreGraphics
import Foundation

/// Where a point in a colour wheel sits, as three channel values.
///
/// Separated from the view for the same reason `SliderGeometry` is: this is the
/// part with arithmetic in it, and arithmetic inside a `GeometryReader` cannot
/// be tested. Every division is guarded — a view is laid out at zero size for
/// at least one pass, and a NaN that reaches a `Shape` is a control that never
/// draws again.
///
/// The channels sit where Resolve puts them: red up, green at two hundred and
/// ten degrees, blue at three hundred and thirty. Pulling towards one raises it
/// and lowers the other two by half as much each, so the three always sum to
/// nothing — a colour wheel moves hue and leaves level alone, and the ribbed
/// bar beside it is what moves level.
public struct WheelGeometry {
    public let bounds: Bounds
    public let radius: CGFloat

    public static let redAngle: Double = 90
    public static let greenAngle: Double = 210
    public static let blueAngle: Double = 330

    public init(bounds: Bounds, radius: CGFloat) {
        self.bounds = bounds
        self.radius = max(0, radius)
    }

    /// How far a push at the rim moves a channel.
    ///
    /// A quarter of the range, not the whole of it: a wheel is for the small
    /// adjustments that make a grade, and one that swung a channel from end to
    /// end across fifty points of travel would be unusable for them. Gain's
    /// range runs to sixteen, and nobody nudges a wheel expecting four.
    private var reach: Float {
        (bounds.max - bounds.min) / 4
    }

    private func offset(at point: CGPoint) -> (angle: Double, amount: Float) {
        guard radius > 0 else { return (0, 0) }
        let dx = Double(point.x)
        let dy = Double(point.y)
        let distance = min((dx * dx + dy * dy).squareRoot(), Double(radius))
        guard distance > 0 else { return (0, 0) }
        var angle = atan2(dy, dx) * 180 / .pi
        if angle < 0 { angle += 360 }
        return (angle, Float(distance / Double(radius)) * reach)
    }

    /// The three channels for a point, measured from the centre.
    public func rgb(at point: CGPoint) -> [Float] {
        let (angle, amount) = offset(at: point)
        guard amount != 0 else {
            return [bounds.neutral, bounds.neutral, bounds.neutral]
        }
        // Each channel gets the cosine of its own angle away from the push,
        // which peaks at the channel being pulled towards and is negative on
        // the far side. Cosines a hundred and twenty degrees apart sum to zero,
        // which is what keeps the level still.
        return [Self.redAngle, Self.greenAngle, Self.blueAngle].map { channel in
            let d = (angle - channel) * .pi / 180
            return bounds.neutral + amount * Float(cos(d))
        }
    }

    /// The inverse: where the handle sits for a set of channel values.
    public func point(for rgb: [Float]) -> CGPoint {
        guard rgb.count == 3, radius > 0 else { return .zero }
        // Sum the three channel vectors. Two thirds because each channel's
        // contribution was a cosine, and three cosines average to two thirds of
        // the amplitude.
        var x = 0.0
        var y = 0.0
        for (value, channel) in zip(rgb, [Self.redAngle, Self.greenAngle, Self.blueAngle]) {
            let magnitude = Double(value - bounds.neutral)
            x += magnitude * cos(channel * .pi / 180)
            y += magnitude * sin(channel * .pi / 180)
        }
        let scale = Double(radius) / Double(max(reach, 1e-6)) * (2.0 / 3.0)
        return CGPoint(x: x * scale, y: y * scale)
    }

    /// A point at an angle and a distance from the centre, for the tests and
    /// for drawing the handle.
    public func point(forAngle degrees: Double, distance: CGFloat) -> CGPoint {
        let d = min(distance, radius)
        let r = degrees * .pi / 180
        return CGPoint(x: CGFloat(cos(r)) * d, y: CGFloat(sin(r)) * d)
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | LC_ALL=C grep -aE "error:|\*\* TEST|Executed [0-9]+ test"
```

Expected: `** TEST SUCCEEDED **`, 37 tests.

If `testAValueRoundTripsBackToAPoint` fails, the two-thirds factor in
`point(for:)` is the thing to check — it is the inverse of averaging three
cosines and is the only constant in the file that is not obvious. Report the
measured values rather than adjusting the tolerance.

- [ ] **Step 5: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "A colour wheel's arithmetic, in a type a test can hold"
```

---

## Task 4: The wheel itself

**Files:**
- Create: `apps/apple/KromaKit/Controls/WheelView.swift`
- Modify: `apps/apple/KromaKit/Engine.swift`, `apps/apple/KromaKit/SessionStore.swift`
- Modify: `apps/apple/KromaKit/InspectorPanel.swift`

- [ ] **Step 1: Give the store a way to set one**

`pe_session_set_wheel(s, row, key, master, r, g, b)` already exists in the C ABI.
Add to `apps/apple/KromaKit/Engine.swift`, in the parameters section:

```swift
    public func setWheel(
        row: UInt64, key: String, master: Float, _ r: Float, _ g: Float, _ b: Float
    ) throws {
        try check(key.withCString {
            pe_session_set_wheel(handle, row, $0, master, r, g, b)
        })
    }
```

And to `apps/apple/KromaKit/SessionStore.swift`, beside `setFloat`:

```swift
    /// The wheel's hot path. Like `setFloat`, it does not refresh the snapshot
    /// mid-drag — the control holds the in-flight value and draws from that.
    public func setWheel(
        row: UInt64, key: String, master: Float, _ r: Float, _ g: Float, _ b: Float
    ) {
        run { try session.setWheel(row: row, key: key, master: master, r, g, b) }
        if !dragging { refresh() }
    }
```

- [ ] **Step 2: Write the view**

Create `apps/apple/KromaKit/Controls/WheelView.swift`:

```swift
import SwiftUI

/// Resolve's four-way wheel: a colour disc with a handle, and a ribbed bar
/// under it for the achromatic push.
///
/// `master` on the registry entry says whether the wheel has a fourth
/// *readout*, not whether it has an achromatic control — every wheel has the
/// bar. Offset is the case that makes the distinction: Resolve draws four bars
/// and three of Offset's boxes, and on a wheel with no master the bar moves the
/// three channels together.
public struct WheelView: View {
    let param: Param
    let bounds: Bounds
    let hasMaster: Bool
    let row: UInt64
    let value: WheelValue
    let isActive: Bool
    let store: SessionStore

    @State private var dragging: WheelValue?

    private var shown: WheelValue { dragging ?? value }

    public init(
        param: Param, bounds: Bounds, hasMaster: Bool, row: UInt64,
        value: WheelValue, isActive: Bool, store: SessionStore
    ) {
        self.param = param
        self.bounds = bounds
        self.hasMaster = hasMaster
        self.row = row
        self.value = value
        self.isActive = isActive
        self.store = store
    }

    private static let size: CGFloat = 84

    public var body: some View {
        VStack(spacing: 4) {
            Text(param.name)
                .font(.caption)
                .foregroundStyle(isActive ? .secondary : .tertiary)

            disc

            bar

            Text(readout)
                .font(.caption2)
                .monospacedDigit()
                .foregroundStyle(isActive ? .secondary : .tertiary)
        }
        .frame(width: Self.size + 16)
        .disabled(!isActive)
    }

    private var readout: String {
        // The master's box, where there is one, and otherwise the three
        // channels — which is what Resolve shows and for the same reason: the
        // bar is a nudge you make without looking, the box is a value you read.
        if hasMaster {
            return String(format: "%.3f", shown.master)
        }
        return shown.rgb.map { String(format: "%.2f", $0) }.joined(separator: " ")
    }

    private var disc: some View {
        GeometryReader { geo in
            let radius = min(geo.size.width, geo.size.height) / 2
            let g = WheelGeometry(bounds: bounds, radius: radius)
            let centre = CGPoint(x: geo.size.width / 2, y: geo.size.height / 2)
            let handle = g.point(for: shown.rgb)

            ZStack {
                Circle()
                    .fill(
                        AngularGradient(
                            colors: [.red, .yellow, .green, .cyan, .blue, .purple, .red],
                            center: .center
                        )
                    )
                    .opacity(isActive ? 0.55 : 0.2)
                Circle().strokeBorder(.quaternary, lineWidth: 1)
                Circle()
                    .fill(.white)
                    .frame(width: 7, height: 7)
                    .overlay(Circle().strokeBorder(.black.opacity(0.6), lineWidth: 1))
                    .position(x: centre.x + handle.x, y: centre.y - handle.y)
            }
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil { store.beginInteraction(param.name) }
                        // Y is flipped: the view grows downward, the wheel does
                        // not.
                        let p = CGPoint(
                            x: drag.location.x - centre.x,
                            y: centre.y - drag.location.y
                        )
                        let rgb = g.rgb(at: p)
                        let next = WheelValue(rgb: rgb, master: shown.master)
                        dragging = next
                        store.setWheel(
                            row: row, key: param.key, master: next.master,
                            rgb[0], rgb[1], rgb[2]
                        )
                    }
                    .onEnded { _ in
                        store.endInteraction()
                        dragging = nil
                    }
            )
        }
        .frame(width: Self.size, height: Self.size)
    }

    /// The achromatic bar. On a wheel with no master it moves the three
    /// channels together, which is what Resolve does with Offset.
    private var bar: some View {
        GeometryReader { geo in
            let g = SliderGeometry(bounds: bounds, width: geo.size.width)
            let position = g.position(of: hasMaster ? shown.master : shown.rgb[0])
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary).frame(height: 4)
                Circle()
                    .fill(.primary)
                    .frame(width: 8, height: 8)
                    .offset(x: position - 4)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if dragging == nil { store.beginInteraction(param.name) }
                        let v = g.value(at: drag.location.x)
                        let next = hasMaster
                            ? WheelValue(rgb: shown.rgb, master: v)
                            : WheelValue(rgb: [v, v, v], master: v)
                        dragging = next
                        store.setWheel(
                            row: row, key: param.key, master: next.master,
                            next.rgb[0], next.rgb[1], next.rgb[2]
                        )
                    }
                    .onEnded { _ in
                        store.endInteraction()
                        dragging = nil
                    }
            )
        }
        .frame(width: Self.size, height: 12)
    }
}
```

- [ ] **Step 3: Route it from the panel**

Wheels do not belong in the four-column row — they are square and sit side by
side, four across a panel. In `InspectorPanel.swift`, add a case that collects
them, and lay them out in a row above the other controls. Replace the `body`
with:

```swift
    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(effect.name)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, 2)

            if !wheels.isEmpty {
                HStack(alignment: .top, spacing: 4) {
                    ForEach(wheels) { param in
                        wheel(param)
                    }
                }
                .padding(.bottom, 4)
            }

            ForEach(effect.params.filter { !isWheel($0) }) { param in
                control(for: param)
            }
        }
        .padding(.vertical, 6)
    }

    private func isWheel(_ param: Param) -> Bool {
        if case .wheel = param.kind { return true }
        return false
    }

    private var wheels: [Param] {
        effect.params.filter(isWheel)
    }

    @ViewBuilder
    private func wheel(_ param: Param) -> some View {
        if case let .wheel(bounds, master) = param.kind {
            WheelView(
                param: param,
                bounds: bounds,
                hasMaster: master,
                row: row.id,
                value: row.params[param.key]?.wheelValue
                    ?? WheelValue(rgb: [bounds.default, bounds.default, bounds.default],
                                  master: bounds.default),
                isActive: effect.isActive(param.key, values: row.params),
                store: store
            )
        }
    }
```

- [ ] **Step 4: Build, run, and check the two wheel panels**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme PhotoEditor -configuration Debug build CODE_SIGNING_ALLOWED=NO 2>&1 | tail -3
```

Expected: `** BUILD SUCCEEDED **`, then the 37 tests still pass, then the app
runs for six seconds printing nothing.

**What a human should see:** Lift / Gamma / Gain showing four wheels across, and
Log Wheels showing four more, where both panels previously read "not yet" four
times. Dragging a wheel towards red should warm the picture without changing how
bright it is.

- [ ] **Step 5: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "Two pinned panels stop saying not yet, and the wheels turn"
```

---

## Task 5: Zoom and pan

The viewer fits the whole frame and cannot do anything else, which makes the
application unusable for the thing it is for: you cannot judge sharpening, or
grain, or a speck of dirt, at a size the screen chose for you.

`Session` already derives its render `Region` from a view state; there is just
no way to move it. This adds one C function and the gesture that drives it.

**Files:**
- Modify: `crates/pe-session/src/session.rs`
- Modify: `crates/pe-ffi/src/lib.rs`
- Create: `apps/apple/KromaKit/ViewState.swift`
- Create: `apps/apple/KromaKitTests/ViewStateTests.swift`
- Modify: `apps/apple/KromaKit/Engine.swift`, `SessionStore.swift`, `MetalViewer.swift`

- [ ] **Step 1: Write the failing test for the arithmetic**

Create `apps/apple/KromaKitTests/ViewStateTests.swift`:

```swift
import XCTest
// Same module as the code under test; see EngineTests.swift.

final class ViewStateTests: XCTestCase {
    func testAFittedViewShowsTheWholeFrame() {
        var v = ViewState()
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
        XCTAssertEqual(v.pan.x, 0, accuracy: 0.0001)
        XCTAssertEqual(v.pan.y, 0, accuracy: 0.0001)
        v.fit()
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
    }

    func testZoomingHoldsThePointUnderTheCursorStill() {
        // The whole reason to anchor a zoom: the pixel you are looking at is
        // the one you want to keep looking at. A zoom that anchors at the
        // centre walks the thing you were inspecting off the screen.
        var v = ViewState()
        let cursor = CGPoint(x: 0.25, y: 0.75)
        let before = v.frameLocation(of: cursor)
        v.zoom(by: 2.5, at: cursor)
        let after = v.frameLocation(of: cursor)
        XCTAssertEqual(after.x, before.x, accuracy: 0.001)
        XCTAssertEqual(after.y, before.y, accuracy: 0.001)
    }

    func testZoomStopsAtBothEnds() {
        // Out beyond fit is a picture floating in a void; in beyond thirty-two
        // is a single pixel filling a screen. Neither is a view of anything.
        var v = ViewState()
        v.zoom(by: 0.001, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.zoom, 1, accuracy: 0.0001)
        v.zoom(by: 10_000, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.zoom, ViewState.maxZoom, accuracy: 0.0001)
    }

    func testPanningCannotPushTheFrameOffScreen()  {
        // At any zoom the visible rectangle stays inside the picture, so there
        // is never a band of nothing along an edge.
        var v = ViewState()
        v.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        v.pan(by: CGSize(width: 100, height: 100))
        XCTAssertGreaterThanOrEqual(v.region.origin.x, 0)
        XCTAssertGreaterThanOrEqual(v.region.origin.y, 0)
        XCTAssertLessThanOrEqual(v.region.maxX, 1.0001)
        XCTAssertLessThanOrEqual(v.region.maxY, 1.0001)
    }

    func testAtFitThereIsNothingToPan() {
        var v = ViewState()
        v.pan(by: CGSize(width: 250, height: -80))
        XCTAssertEqual(v.region.origin.x, 0, accuracy: 0.0001)
        XCTAssertEqual(v.region.origin.y, 0, accuracy: 0.0001)
        XCTAssertEqual(v.region.width, 1, accuracy: 0.0001)
    }

    func testTheRegionShrinksAsYouZoomIn() {
        var v = ViewState()
        v.zoom(by: 4, at: CGPoint(x: 0.5, y: 0.5))
        XCTAssertEqual(v.region.width, 0.25, accuracy: 0.0001)
        XCTAssertEqual(v.region.height, 0.25, accuracy: 0.0001)
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | tail -15
```

Expected: compile failure, `cannot find 'ViewState' in scope`.

- [ ] **Step 3: Write the arithmetic**

Create `apps/apple/KromaKit/ViewState.swift`:

```swift
import CoreGraphics

/// How much of the photograph is on screen, and which part.
///
/// Kept here rather than in the engine because it is a property of the window,
/// not of the document — two windows on one photograph would disagree about it
/// and both be right. The engine is told the answer and renders that rectangle;
/// it does not have an opinion about scroll wheels.
///
/// Everything is in frame coordinates, where the whole picture is the unit
/// square, so none of it depends on how big the window happens to be.
public struct ViewState: Equatable {
    /// One is the whole frame fitted. Thirty-two is as far in as it goes —
    /// beyond that a pixel fills a screen and there is nothing left to judge.
    public static let maxZoom: CGFloat = 32

    public private(set) var zoom: CGFloat = 1
    public private(set) var pan: CGPoint = .zero

    public init() {}

    public mutating func fit() {
        zoom = 1
        pan = .zero
    }

    /// The visible rectangle, in frame coordinates.
    public var region: CGRect {
        let size = 1 / zoom
        // Clamped so the rectangle never leaves the picture. At fit there is
        // nowhere to go and the clamp collapses to zero, which is why panning a
        // fitted view does nothing.
        let slack = max(0, 1 - size)
        let x = min(max(pan.x, 0), slack)
        let y = min(max(pan.y, 0), slack)
        return CGRect(x: x, y: y, width: size, height: size)
    }

    /// Where a point of the *view* lands in the frame. The view point is a
    /// fraction of the viewport, so (0.5, 0.5) is its middle.
    public func frameLocation(of viewPoint: CGPoint) -> CGPoint {
        let r = region
        return CGPoint(
            x: r.origin.x + viewPoint.x * r.width,
            y: r.origin.y + viewPoint.y * r.height
        )
    }

    /// Zoom about a point of the view, holding whatever is under it still.
    public mutating func zoom(by factor: CGFloat, at viewPoint: CGPoint) {
        let anchor = frameLocation(of: viewPoint)
        zoom = min(max(zoom * factor, 1), Self.maxZoom)
        // Put the anchor back under the same point of the view.
        let size = 1 / zoom
        pan = CGPoint(
            x: anchor.x - viewPoint.x * size,
            y: anchor.y - viewPoint.y * size
        )
        normalise()
    }

    /// Drag the picture. The delta is in *view* fractions, so a drag across
    /// half the window moves half of whatever is on screen, at any zoom.
    public mutating func pan(by delta: CGSize) {
        let size = 1 / zoom
        pan = CGPoint(
            x: pan.x - delta.width * size,
            y: pan.y - delta.height * size
        )
        normalise()
    }

    private mutating func normalise() {
        let r = region
        pan = r.origin
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | LC_ALL=C grep -aE "error:|\*\* TEST|Executed [0-9]+ test"
```

Expected: `** TEST SUCCEEDED **`, 43 tests.

- [ ] **Step 5: Give the engine somewhere to put it**

In `crates/pe-session/src/session.rs`, add a field to `Session` beside
`open_set`:

```rust
    /// Which rectangle of the frame the viewer is showing. A property of the
    /// window rather than of the document: two windows on one photograph would
    /// disagree about it and both be right.
    view: Region,
```

initialised to `Region::FULL` in `Session::new`, and add to `impl Session` in
the rendering section:

```rust
    /// Show this rectangle of the frame, in frame coordinates.
    ///
    /// The working texture is built for a particular rectangle, so moving it
    /// invalidates that texture and every cached stage that reads it — which
    /// the stage cache already knows, because `Region` is part of its key.
    pub fn set_view(&mut self, x: f32, y: f32, size: f32) {
        let size = size.clamp(1.0 / 32.0, 1.0);
        let region = Region {
            offset: [
                x.clamp(0.0, 1.0 - size),
                y.clamp(0.0, 1.0 - size),
            ],
            size: [size, size],
        };
        if region != self.view {
            self.view = region;
            self.gpu.working_size = (0, 0);
            self.needs_render = true;
        }
    }
```

Then thread it through. `graded` is shared by the screen and by
`render_offscreen`, and **an export is never a partial view** — so the region
becomes an argument rather than something `graded` reads off the field, and each
caller says what it wants. Change the signature:

```rust
    fn graded(
        &mut self,
        width: u32,
        height: u32,
        region: Region,
    ) -> Result<wgpu::TextureView, SessionError> {
```

Inside it, use `region` in the two places that currently say `Region::FULL`:

```rust
            let sampling = Sampling::of(&geometry, photo.image.width, photo.image.height)
                .within(region);
```

```rust
        renderer.set_region(region);
```

The working texture is rebuilt when the region changes, so add `region` to the
guard beside size and geometry — a texture built for one rectangle is wrong for
another:

```rust
        if self.gpu.working_size != (width, height)
            || self.gpu.working_geometry != Some(geometry)
            || self.gpu.working_region != Some(region)
        {
```

with a `working_region: Option<Region>` field on `Gpu`, set alongside the other
two and cleared to `None` wherever they are.

Then `present` passes `self.view`, and `render_offscreen` passes `Region::FULL`
— which is the whole point of making it an argument.

- [ ] **Step 6: Write the failing Rust test**

`render_offscreen` deliberately renders the whole frame, so it cannot observe
the view — which leaves the view's *state* as the thing to test, and that is the
right level for it anyway. Pixels are the screen's business and there is no
screen in a unit test.

Add to the test module in `crates/pe-session/src/session.rs`:

```rust
    #[test]
    fn moving_the_view_invalidates_the_texture_built_for_the_old_one() {
        // The working texture is built for a particular rectangle of the frame.
        // Leaving it in place after the view moves would show the previous
        // rectangle, scaled — the picture would appear to zoom and then not
        // resolve.
        let mut s = chart_session();
        s.render_offscreen(64, 64).unwrap();
        assert!(!s.needs_render());

        s.set_view(0.25, 0.25, 0.25);
        assert!(s.needs_render(), "moving the view did not ask for a frame");
        assert_eq!(
            s.view_region(),
            (0.25, 0.25, 0.25),
            "the view did not go where it was sent"
        );
    }

    #[test]
    fn a_view_cannot_be_pushed_off_the_frame() {
        // Clamped, so there is never a band of nothing along an edge.
        let mut s = chart_session();
        s.set_view(5.0, -5.0, 0.5);
        let (x, y, size) = s.view_region();
        assert_eq!(size, 0.5);
        assert!((0.0..=0.5).contains(&x), "x escaped the frame: {x}");
        assert!((0.0..=0.5).contains(&y), "y escaped the frame: {y}");
    }

    #[test]
    fn a_view_cannot_zoom_past_a_single_pixel_of_use() {
        let mut s = chart_session();
        s.set_view(0.0, 0.0, 0.0001);
        assert_eq!(s.view_region().2, 1.0 / 32.0);
    }

    #[test]
    fn an_export_renders_the_whole_frame_however_the_viewer_is_zoomed() {
        // The one that would be a real bug: exporting what is on screen rather
        // than what is in the file.
        let mut s = chart_session();
        let fitted = s.render_offscreen(64, 64).unwrap();
        s.set_view(0.25, 0.25, 0.25);
        let zoomed = s.render_offscreen(64, 64).unwrap();
        assert_eq!(fitted, zoomed, "the export followed the viewer");
    }
```

That last test needs `render_offscreen` to pass `Region::FULL`, which Step 5
already had it do. Add the accessor it reads:

```rust
    /// The visible rectangle, as the shell gave it: x, y and size in frame
    /// coordinates.
    pub fn view_region(&self) -> (f32, f32, f32) {
        (self.view.offset[0], self.view.offset[1], self.view.size[0])
    }
```

- [ ] **Step 7: Expose it across the C ABI**

In `crates/pe-ffi/src/lib.rs`, in the render section:

```rust
/// Show this rectangle of the frame. `size` is the fraction of the whole
/// picture that is visible, so 1.0 is fitted and 0.25 is four times in.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_view(
    s: *mut PeSession,
    x: f32,
    y: f32,
    size: f32,
) -> i32 {
    status(s, move |s| {
        s.set_view(x, y, size);
        Ok(())
    })
}
```

- [ ] **Step 8: Wire the Swift side**

`apps/apple/KromaKit/Engine.swift`, in the screen section:

```swift
    public func setView(x: Float, y: Float, size: Float) throws {
        try check(pe_session_set_view(handle, x, y, size))
    }
```

`apps/apple/KromaKit/SessionStore.swift` — hold the view and push it down:

```swift
    /// Where the viewer is looking. Held here rather than in the engine
    /// because it belongs to the window, not to the photograph.
    public private(set) var view = ViewState()

    public func zoom(by factor: CGFloat, at viewPoint: CGPoint) {
        view.zoom(by: factor, at: viewPoint)
        pushView()
    }

    public func pan(by delta: CGSize) {
        view.pan(by: delta)
        pushView()
    }

    public func fitView() {
        view.fit()
        pushView()
    }

    private func pushView() {
        let r = view.region
        run { try session.setView(x: Float(r.origin.x), y: Float(r.origin.y), size: Float(r.width)) }
    }
```

Add `import CoreGraphics` if it is not already there.

`apps/apple/KromaKit/MetalViewer.swift` — the gestures. Add to `MetalViewerView`:

```swift
    public override func scrollWheel(with event: NSEvent) {
        // Scroll zooms, anchored under the cursor, which is what every editor
        // that is any good does and what the Windows shell does.
        let point = convert(event.locationInWindow, from: nil)
        let anchor = CGPoint(
            x: bounds.width > 0 ? point.x / bounds.width : 0.5,
            // Flipped: the view grows downward, the frame does not.
            y: bounds.height > 0 ? 1 - point.y / bounds.height : 0.5
        )
        let factor = 1 + event.scrollingDeltaY * 0.01
        store.zoom(by: factor, at: anchor)
    }

    public override func mouseDragged(with event: NSEvent) {
        guard bounds.width > 0, bounds.height > 0 else { return }
        store.pan(by: CGSize(
            width: event.deltaX / bounds.width,
            height: -event.deltaY / bounds.height
        ))
    }

    public override func mouseDown(with event: NSEvent) {
        // Double-click fits, as it does on the Windows side.
        if event.clickCount == 2 {
            store.fitView()
        }
    }

    public override var acceptsFirstResponder: Bool { true }
```

- [ ] **Step 9: Verify everything**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning: unused"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:"
```

Expected: fmt and clippy silent, 611 passed / 0 failed (607 plus the four new
session tests). Then the Swift suite at 43, then the app running for six seconds
printing nothing.

- [ ] **Step 10: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A crates apps/apple && git commit -m "You can get close enough to see the grain"
```

---

## Task 6: Every effect becomes reachable

The largest gap between the two shells, and not a control kind at all. The Mac
app draws the eleven pinned rows and nothing else, because there is no way to
add a row — so nineteen of the thirty effects cannot be used, including every
Film and Optics effect the application is partly named for.

All the engine calls exist already: `pe_session_add_effect`,
`pe_session_remove_row`, `pe_session_move_row`, `pe_session_set_row_enabled`,
`pe_session_set_row_opacity`.

**Files:**
- Create: `apps/apple/KromaKit/EffectBrowser.swift`
- Create: `apps/apple/KromaKit/StackRowView.swift`
- Modify: `apps/apple/KromaKit/SessionStore.swift`
- Modify: `apps/apple/PhotoEditor/ContentView.swift`
- Modify: `apps/apple/KromaKitTests/SessionStoreTests.swift`

- [ ] **Step 1: Write the failing test**

Add to `SessionStoreTests`:

```swift
    func testARowCanBeAddedRemovedAndReordered() throws {
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let pinned = store.snapshot.rows.count

        let grain = try XCTUnwrap(store.addEffect("grain"))
        XCTAssertEqual(store.snapshot.rows.count, pinned + 1)
        XCTAssertEqual(store.snapshot.rows.last?.effect, "grain")

        let halation = try XCTUnwrap(store.addEffect("halation"))
        XCTAssertEqual(store.snapshot.rows.count, pinned + 2)

        // Reordering moves it within the stack, and the stack is the document —
        // grain under halation is a different photograph from halation under
        // grain.
        store.moveRow(halation, to: UInt32(pinned))
        let order = store.snapshot.rows.map(\.effect)
        XCTAssertLessThan(
            order.firstIndex(of: "halation")!,
            order.firstIndex(of: "grain")!
        )

        store.removeRow(grain)
        XCTAssertEqual(store.snapshot.rows.count, pinned + 1)
        XCTAssertFalse(store.snapshot.rows.contains { $0.effect == "grain" })
    }

    func testARowCanBeSwitchedOffWithoutBeingRemoved() throws {
        // Bypassing a row is how you find out what it was doing. Removing it
        // and adding it back is not the same thing — it loses the parameters.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let row = try XCTUnwrap(store.addEffect("grain"))
        store.setFloat(row: row, key: "amount", value: 0.5)

        store.setRowEnabled(row, false)
        XCTAssertEqual(store.snapshot.rows.first { $0.id == row }?.enabled, false)

        store.setRowEnabled(row, true)
        let back = try XCTUnwrap(store.snapshot.rows.first { $0.id == row })
        XCTAssertTrue(back.enabled)
        XCTAssertEqual(back.params["amount"]?.floatValue, 0.5, accuracy: 0.0001)
    }

    func testAPinnedRowIsNotOfferedForRemoval() throws {
        // The pinned rows are the fixed panels of the colour page. Removing one
        // would leave a document a fresh one could not be, and the inspector
        // with a hole in it.
        let store = try XCTUnwrap(SessionStore())
        store.openTestChart()
        let pinned = try XCTUnwrap(store.snapshot.rows.first { $0.pinned })
        XCTAssertFalse(store.canRemove(pinned))

        let added = try XCTUnwrap(store.addEffect("grain"))
        let row = try XCTUnwrap(store.snapshot.rows.first { $0.id == added })
        XCTAssertTrue(store.canRemove(row))
    }
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | tail -15
```

Expected: compile failure — `moveRow`, `removeRow`, `setRowEnabled` and
`canRemove` are not on `SessionStore`.

- [ ] **Step 3: Add them to the store**

In `apps/apple/KromaKit/SessionStore.swift`, beside `addEffect`:

```swift
    public func removeRow(_ row: UInt64) {
        run { try session.removeRow(row) }
        refresh()
    }

    public func moveRow(_ row: UInt64, to index: UInt32) {
        run { try session.moveRow(row, to: index) }
        refresh()
    }

    public func setRowOpacity(_ row: UInt64, _ value: Float) {
        run { try session.setRowOpacity(row, value) }
        if !dragging { refresh() }
    }

    /// Whether this row may be taken out of the stack.
    ///
    /// The pinned rows are the colour page's fixed panels; a document without
    /// them is one a fresh document could not be, and an inspector with a hole
    /// in it. The engine would allow it, which is why the answer lives here
    /// rather than being assumed.
    public func canRemove(_ row: Snapshot.Row) -> Bool {
        !row.pinned
    }
```

- [ ] **Step 4: Run the tests**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild test -project PhotoEditor.xcodeproj -scheme KromaKitTests -destination 'platform=macOS' CODE_SIGNING_ALLOWED=NO 2>&1 | LC_ALL=C grep -aE "error:|\*\* TEST|Executed [0-9]+ test"
```

Expected: `** TEST SUCCEEDED **`, 46 tests.

- [ ] **Step 5: Write the browser**

Create `apps/apple/KromaKit/EffectBrowser.swift`:

```swift
import SwiftUI

/// The menu that adds a row, grouped as the registry groups them.
///
/// Nothing here lists an effect by name. `Group::ALL` exists in Rust so that
/// adding a variant and forgetting to list it is a compile error rather than an
/// effect that is fully implemented, has a shader, passes its tests — and
/// cannot be added to a stack, because nothing draws a heading for it. The same
/// property holds on this side by generating the menu from `registry.groups`.
public struct EffectBrowser: View {
    let registry: Registry
    let store: SessionStore

    public init(registry: Registry, store: SessionStore) {
        self.registry = registry
        self.store = store
    }

    public var body: some View {
        Menu {
            ForEach(registry.groups, id: \.self) { group in
                Section(group) {
                    ForEach(addable(in: group)) { effect in
                        Button(effect.name) {
                            store.addEffect(effect.key)
                        }
                    }
                }
            }
        } label: {
            Label("Add effect", systemImage: "plus")
        }
        .menuStyle(.borderlessButton)
        .disabled(!store.snapshot.isOpen)
    }

    /// Everything in a group except the pinned rows, which are already in every
    /// document and would do nothing useful twice.
    private func addable(in group: String) -> [Effect] {
        registry.effects.filter { $0.group == group && !registry.pinned.contains($0.key) }
    }
}
```

- [ ] **Step 6: Write the row**

Create `apps/apple/KromaKit/StackRowView.swift`:

```swift
import SwiftUI

/// One added row: what it is, whether it is doing anything, how much of it, and
/// the buttons that move or remove it.
///
/// Every row carries its own `enabled`, `opacity` and `blend`, mirroring a
/// Resolve node's anatomy — so "grain at forty per cent in Screen mode" is
/// three fields that already exist rather than a feature built into grain.
public struct StackRowView: View {
    let effect: Effect
    let row: Snapshot.Row
    let index: Int
    let count: Int
    let store: SessionStore

    @State private var dragging: Float?

    public init(
        effect: Effect, row: Snapshot.Row, index: Int, count: Int, store: SessionStore
    ) {
        self.effect = effect
        self.row = row
        self.index = index
        self.count = count
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                Toggle("", isOn: Binding(
                    get: { row.enabled },
                    set: { store.setRowEnabled(row.id, $0) }
                ))
                .labelsHidden()
                .toggleStyle(.checkbox)
                .help("Bypass this row")

                Text(effect.name)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(row.enabled ? .secondary : .tertiary)

                Spacer()

                Button {
                    store.moveRow(row.id, to: UInt32(max(0, index - 1)))
                } label: {
                    Image(systemName: "chevron.up")
                }
                .buttonStyle(.borderless)
                .disabled(index == 0)

                Button {
                    store.moveRow(row.id, to: UInt32(min(count - 1, index + 1)))
                } label: {
                    Image(systemName: "chevron.down")
                }
                .buttonStyle(.borderless)
                .disabled(index >= count - 1)

                if store.canRemove(row) {
                    Button(role: .destructive) {
                        store.removeRow(row.id)
                    } label: {
                        Image(systemName: "trash")
                    }
                    .buttonStyle(.borderless)
                }
            }

            // Opacity, as the same four-column row every parameter uses, so the
            // columns line up with the controls underneath it.
            HStack(spacing: RowMetrics.gap) {
                Text("Blend")
                    .frame(width: RowMetrics.label, alignment: .trailing)
                    .foregroundStyle(.tertiary)
                GeometryReader { geo in
                    let bounds = Bounds(min: 0, max: 1, default: 1, neutral: 1)
                    let g = SliderGeometry(bounds: bounds, width: geo.size.width)
                    let shown = dragging ?? row.opacity
                    ZStack(alignment: .leading) {
                        Capsule().fill(.quaternary).frame(height: 3)
                        Circle()
                            .fill(.primary)
                            .frame(width: 8, height: 8)
                            .offset(x: g.position(of: shown) - 4)
                    }
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { drag in
                                if dragging == nil { store.beginInteraction("Opacity") }
                                let v = g.value(at: drag.location.x)
                                dragging = v
                                store.setRowOpacity(row.id, v)
                            }
                            .onEnded { _ in
                                store.endInteraction()
                                dragging = nil
                            }
                    )
                }
                Text(String(format: "%.0f%%", (dragging ?? row.opacity) * 100))
                    .frame(width: RowMetrics.value, alignment: .trailing)
                    .monospacedDigit()
                    .foregroundStyle(.tertiary)
            }
            .frame(height: RowMetrics.height)
            .font(.caption)
        }
    }
}
```

- [ ] **Step 7: Put both in the inspector**

In `apps/apple/PhotoEditor/ContentView.swift`, replace `inspector` with:

```swift
    /// The pinned panels, then everything that has been added, then the button
    /// that adds more.
    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(store.snapshot.rows.enumerated()), id: \.element.id) { index, row in
                    if let effect = store.registry.effect(row.effect) {
                        if !row.pinned {
                            StackRowView(
                                effect: effect,
                                row: row,
                                index: index,
                                count: store.snapshot.rows.count,
                                store: store
                            )
                        }
                        InspectorPanel(effect: effect, row: row, store: store)
                        Divider()
                    }
                }

                EffectBrowser(registry: store.registry, store: store)
                    .padding(.vertical, 8)
            }
            .padding(.horizontal, 8)
        }
    }
```

- [ ] **Step 8: Build, run, and look**

```bash
cd "/Volumes/Projects/Programming/photo editor/apps/apple" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && xcodegen generate && xcodebuild -project PhotoEditor.xcodeproj -scheme PhotoEditor -configuration Debug build CODE_SIGNING_ALLOWED=NO 2>&1 | tail -3
```

Expected: `** BUILD SUCCEEDED **`, 46 tests still passing, app alive for six
seconds printing nothing.

**What a human should see:** an "Add effect" menu at the bottom of the inspector
listing four groups — Basic, Colour, Film, Optics — with the nineteen effects
that are not already pinned. Adding Grain should put a row at the bottom with a
bypass box, arrows, a bin, an opacity slider and its own parameters underneath.

- [ ] **Step 9: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "The other nineteen effects stop being unreachable"
```

---

## Task 7: The documents say what is drawn and what is not

**Files:**
- Modify: `README.md`, `apps/apple/README.md`

- [ ] **Step 1: Correct the Apple README**

Replace the `PhotoEditor` row of the Targets table:

```markdown
| `PhotoEditor` | The macOS application. Opens a photograph, adds and reorders effects, grades through the pinned panels and anything added to the stack, zooms, undoes, autosaves and exports. Curves, the Colour Warper's lattices and its pins are not drawn yet; their rows say so. |
```

- [ ] **Step 2: Correct the root README**

The milestone table's M6 row:

```
| M6 | macOS | in progress — six of eight control kinds, every effect reachable |
```

- [ ] **Step 3: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add README.md apps/apple/README.md && git commit -m "The documents describe six control kinds and a reachable stack"
```

---

## Done when

- [ ] `bool`, `choice`, `rgb` and `wheel` all draw, so 317 of 339 parameters have a control — 94 per cent of the registry.
- [ ] Lift / Gamma / Gain and Log Wheels are complete panels rather than four rows saying "not yet" each.
- [ ] Every one of the thirty effects can be added, bypassed, reordered, blended and removed.
- [ ] Scroll zooms anchored under the cursor, drag pans, double-click fits, and the visible rectangle never leaves the picture.
- [ ] `cargo test --workspace --no-fail-fast` at 609 passed / 0 failed; `xcodebuild test -scheme KromaKitTests` at 46.
- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` both silent.

## What this plan deliberately does not do

- **The curve editor.** Ten parameters, and 1,693 lines of drawing in the egui shell — a control with its own hit-testing, its own interpolation preview and a histogram behind it. Its own plan.
- **The Colour Warper's lattices and pins.** Four parameters, 1,021 lines on the Windows side, and three linked views. Its own plan.
- **Scopes, crop, the filmstrip, batch export, compare.** Each is a panel rather than a control, and none is needed to grade one photograph.
- **The eyedropper** on colour parameters, which needs a call that asks the engine what colour a rendered pixel is. There is no such call.
- **Rendering only the visible rectangle at its own resolution.** `set_view` narrows what is rendered, but the working texture is still built at the viewport's size — so zooming in magnifies rather than resolving more detail. Closing that is a change to how the preview picks its render size, and belongs with the filmstrip work where the same question arises.
