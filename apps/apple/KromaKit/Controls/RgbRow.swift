import AppKit
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
