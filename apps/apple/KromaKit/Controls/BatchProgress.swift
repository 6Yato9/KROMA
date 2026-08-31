import SwiftUI

/// The bar a batch export runs behind, and what it says when it stops.
///
/// Nothing at all when there is no run and nothing to report, which is what
/// lets it sit under the window unconditionally the way the filmstrip sits
/// beside it: which session gets a bar is decided here and nowhere else.
///
/// **No photograph is exported in this file.** The stepping is
/// ``SessionStore/stepBatch()``, called once a frame from the display link — a
/// full-resolution render inside a view update is a frozen window with extra
/// steps. What is here reads three numbers and draws them.
public struct BatchProgress: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        if let counts = store.batch {
            running(counts)
        } else if let said = store.batchSummary {
            report(said)
        }
    }

    /// Done, failed and total, all three.
    ///
    /// The two counts do not add up to the third until the run is over, and
    /// what is left is the number somebody waiting on a folder of sixty
    /// actually wants. The failures are only named once there are any: a
    /// standing "0 failed" is a worry offered to everybody who never had one.
    private func running(_ counts: BatchCounts) -> some View {
        bar {
            // Lower case, like every other running line in both shells:
            // `main.rs` writes "exporting n of m" here and "grade copied",
            // "saved n edits" in the status bar. A sentence that starts with a
            // capital reads as a heading rather than as something happening.
            Text("exporting \(counts.done + counts.failed) of \(counts.total)")
                .foregroundStyle(Palette.title.color)
                .monospacedDigit()
            track(counts.fraction)
            if counts.failed > 0 {
                Text("\(counts.failed) failed")
                    .foregroundStyle(Palette.warn.color)
                    .monospacedDigit()
            }
            Spacer(minLength: 0)
            Button("Stop") { store.cancelBatch() }
                .buttonStyle(KromaButtonStyle())
                .help("Stop the run, keeping what it has already written")
        }
    }

    /// `n exported`, or `n exported, m failed`, until it is dismissed.
    ///
    /// It stays up rather than fading with the bar, because a run that finished
    /// and said nothing is indistinguishable from one that crashed on its first
    /// photograph.
    private func report(_ said: String) -> some View {
        bar {
            Text(said)
                .foregroundStyle(Palette.title.color)
            Spacer(minLength: 0)
            Button("Dismiss") { store.dismissBatchSummary() }
                .buttonStyle(KromaButtonStyle())
        }
    }

    /// One filled bar: `TRACK` behind, `ACCENT` in front.
    ///
    /// The accent is spent on what is *doing something*, and a run in progress
    /// is the one thing in the window that is.
    private func track(_ fraction: Double) -> some View {
        GeometryReader { space in
            ZStack(alignment: .leading) {
                Capsule().fill(Palette.track.color)
                Capsule()
                    .fill(Palette.accent.color)
                    .frame(width: space.size.width * min(max(fraction, 0), 1))
            }
        }
        .frame(width: 220, height: 4)
    }

    /// The strip itself: `RAISED`, hairlined off the window above it, the same
    /// furniture the status bar is built from.
    private func bar<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 10, content: content)
            .font(.system(size: 11))
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .frame(maxWidth: .infinity)
            .background(Palette.raised.color)
            .overlay(alignment: .top) { Hairline() }
    }
}
