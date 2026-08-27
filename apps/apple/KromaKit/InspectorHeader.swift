import SwiftUI

/// The inspector's title bar: what is being edited, and how big it is.
///
/// `inspector_header` in `main.rs` — a picture glyph, the name at the left in
/// `TITLE`, the source's size at the right in `LABEL`, and a `RULE` hairline
/// under the lot.
///
/// The same two facts are in the status bar, and deliberately in both places:
/// this says what the column of controls under it belongs to, and the status
/// bar says what the window is showing. Two questions, one answer each.
public struct InspectorHeader: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    /// `main.rs`'s 34.
    static let height: CGFloat = 34

    public var body: some View {
        HStack(spacing: 10) {
            // Windows draws this by hand because its bundled fonts have no
            // dingbats. Here it is a system symbol, for the reason the tab
            // glyphs are: it is the platform idiom and it scales with the type.
            Image(systemName: "photo")
                .imageScale(.medium)
                .foregroundStyle(Palette.icon.color)
            Text(name)
                .font(.system(size: 14))
                .foregroundStyle(Palette.title.color)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(store.snapshot.path ?? name)
            Spacer(minLength: 8)
            Text(size)
                .font(.system(size: 11))
                .foregroundStyle(Palette.label.color)
                .monospacedDigit()
                .fixedSize()
        }
        .padding(.horizontal, 8)
        .frame(height: Self.height)
        .frame(maxWidth: .infinity)
        .background(Palette.raised.color)
        .overlay(alignment: .bottom) { Hairline() }
    }

    /// The same fallback the status bar and `file_page` use, so a session with
    /// no file open is named the same thing everywhere.
    private var name: String {
        store.snapshot.name ?? "test chart"
    }

    /// The *source's* size, which is what Windows puts here. What an export
    /// will be is on the File tab, next to the setting that decides it.
    private var size: String {
        "\(store.snapshot.width) x \(store.snapshot.height)"
    }
}
