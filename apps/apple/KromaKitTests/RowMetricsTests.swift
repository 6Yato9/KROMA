import SwiftUI
import XCTest

/// The row's own arithmetic, and whether a panel of the stated minimum width
/// can actually draw one.
///
/// This file exists because it did not, and the application shipped for weeks
/// with every control in it drawing a clipped label — "Temperature" as
/// "rature", "Exposure" as "osure". The inspector was pinned at 260 points
/// while the label, readout and reset arrow cost 206 between them before the
/// track had a single point, and a `GeometryReader` track takes the width it is
/// offered rather than negotiating down. The fixed parts were pushed outside
/// the frame and clipped at *both* ends.
///
/// Nothing caught it. Every unit test passed, because none of them asked what a
/// row looks like at the width the application actually gives it.
///
/// What these tests pin is the arithmetic and the rendering at the stated
/// minimum. They do **not** reproduce the original overflow, because the
/// minimum width on the track changed what happens below the minimum: a row too
/// narrow now truncates rather than overflowing past both edges. The guard that
/// matters is `testAPanelIsWideEnoughForTheRowItDraws` together with
/// `ContentView` reading `RowMetrics.minimumPanel` instead of carrying a number
/// of its own — the bug was two numbers disagreeing, and there is now one.
final class RowMetricsTests: XCTestCase {
    /// The arithmetic, stated where it can be read.
    func testAPanelIsWideEnoughForTheRowItDraws() {
        XCTAssertGreaterThanOrEqual(
            RowMetrics.minimumPanel - RowMetrics.inset * 2,
            RowMetrics.minimumRow,
            "the panel cannot fit the row it is for"
        )
    }

    /// The track's floor is a floor on usefulness, not on looks: a slider this
    /// short gives a hundredth of its range to about half a point of travel.
    func testTheTrackKeepsEnoughTravelToAim() {
        let hundredth = RowMetrics.track / 100
        XCTAssertGreaterThan(hundredth, 0.5, "a hundredth of the range is unaimable")
    }

    /// The test that would have caught it.
    ///
    /// At the panel's own minimum width a row must leave blank space to the
    /// left of its label. A clipped label runs hard to the left edge, because
    /// the overflow is centred and the front of the word is cut off — which is
    /// exactly what "rature" is.
    @MainActor
    func testALabelIsNotClippedAtTheNarrowestPanel() throws {
        let width = RowMetrics.minimumPanel - RowMetrics.inset * 2
        let image = try Self.render(Self.sampleRow, width: width)
        let firstInk = try XCTUnwrap(
            Self.firstInkedColumn(image), "the row drew nothing at all")
        XCTAssertGreaterThan(
            firstInk, 0,
            "the label runs to the left edge, which is what a clipped label does"
        )
    }

    /// And the readout must not fall off the other end.
    @MainActor
    func testTheReadoutIsNotPushedOffTheRightEdge() throws {
        let width = RowMetrics.minimumPanel - RowMetrics.inset * 2
        let image = try Self.render(Self.sampleRow, width: width)
        let lastInk = try XCTUnwrap(
            Self.lastInkedColumn(image), "the row drew nothing at all")
        XCTAssertLessThan(
            lastInk, image.width - 1,
            "something runs to the right edge, which is what a pushed-out readout does"
        )
    }

    /// Widening the panel must not move the label — it is a fixed column, and a
    /// row that re-centres as the panel grows is a row whose parts are being
    /// laid out by the overflow rather than by the metrics.
    @MainActor
    func testWideningThePanelLeavesTheLabelWhereItWas() throws {
        let narrow = try Self.render(
            Self.sampleRow, width: RowMetrics.minimumPanel - RowMetrics.inset * 2)
        let wide = try Self.render(Self.sampleRow, width: 460)
        let a = try XCTUnwrap(Self.firstInkedColumn(narrow))
        let b = try XCTUnwrap(Self.firstInkedColumn(wide))
        XCTAssertEqual(a, b, accuracy: 3, "the label moved when the panel grew")
    }

    // ---- helpers ---------------------------------------------------------

    /// A row with the longest label the registry actually uses, since that is
    /// the one that clips first.
    @MainActor
    private static var sampleRow: some View {
        ScalarRow(
            name: "Temperature",
            unit: "K",
            value: 5600,
            bounds: Bounds(min: 2000, max: 11000, default: 5600, neutral: 5600),
            isActive: true,
            onChange: { _ in },
            onBegin: {},
            onEnd: {}
        )
    }

    @MainActor
    private static func render<V: View>(_ view: V, width: CGFloat) throws -> CGImage {
        // Dark, because that is what the application is and because `.primary`
        // resolves to black otherwise — black ink on the black ground below
        // would read as no ink at all and the test would pass for the wrong
        // reason.
        let renderer = ImageRenderer(
            content: view
                .frame(width: width, height: RowMetrics.height)
                .background(.black)
                .environment(\.colorScheme, .dark))
        renderer.scale = 1
        return try XCTUnwrap(renderer.cgImage, "the renderer produced no image")
    }

    private static func columns(_ image: CGImage) -> [Bool] {
        let (w, h) = (image.width, image.height)
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        guard
            let context = CGContext(
                data: &bytes, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return [] }
        context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        // A column counts as inked if anything in it is meaningfully brighter
        // than the black ground — the row is drawn on black, so the label, the
        // track and the handle all read as light.
        return (0..<w).map { x in
            (0..<h).contains { y in
                let i = (y * w + x) * 4
                return bytes[i] > 40 || bytes[i + 1] > 40 || bytes[i + 2] > 40
            }
        }
    }

    private static func firstInkedColumn(_ image: CGImage) -> Int? {
        columns(image).firstIndex(of: true)
    }

    private static func lastInkedColumn(_ image: CGImage) -> Int? {
        columns(image).lastIndex(of: true)
    }
}
