import SwiftUI

/// The formats an export may be written as.
///
/// A mirror of `pe_session::export::Format`, checked against it by
/// `export_formats.json`: the same three, in the same order, with the same
/// labels and the same rule about which of them carries a quality.
///
/// The engine's name is the identity here rather than a Swift case, because
/// the name is what crosses the FFI in both directions — the snapshot returns
/// one and ``SessionStore/setExport(format:quality:)`` takes one. A name this
/// build has never heard of is a format a newer engine has, and is shown as
/// itself rather than dropped.
public struct ExportFormat: Sendable, Equatable {
    public let name: String
    public let label: String
    public let takesQuality: Bool

    public static let all: [ExportFormat] = [
        ExportFormat(name: "jpeg", label: "JPEG", takesQuality: true),
        ExportFormat(name: "png", label: "PNG 8", takesQuality: false),
        ExportFormat(name: "png16", label: "PNG 16", takesQuality: false),
    ]

    /// The label for an engine name, or the name itself if it is unknown.
    ///
    /// Never nil, and that is the point: a ``ChoiceMenu`` whose chosen value is
    /// not among its options draws an empty button, so a format this build does
    /// not know still has to name itself.
    public static func label(of name: String) -> String {
        all.first { $0.name == name }?.label ?? name
    }

    /// The engine name for a label the menu just handed back.
    public static func name(ofLabel label: String) -> String {
        all.first { $0.label == label }?.name ?? label
    }

    /// Whether the quality control means anything for this format.
    ///
    /// `pe_session::export::Format::takes_quality`, and an unknown format is
    /// assumed not to take one: the quality is JPEG's idea, and greying a
    /// control that turns out to be live is a smaller wrong than offering a
    /// setting that does nothing.
    public static func takesQuality(name: String) -> Bool {
        all.first { $0.name == name }?.takesQuality ?? false
    }
}

/// The File page: what the photograph is, and what it will be written as.
///
/// `apps/windows`'s fourth tab, which is the last one this shell did not have.
/// Five facts and two settings — and settings rather than a dialog,
/// deliberately: a dialog asks the same question every time and is answered
/// the same way every time, where a panel states the answer, keeps it, and
/// stays out of the way of somebody exporting sixty frames.
///
/// Like ``CropPanel``, nothing here is generated from the registry. There is no
/// `Effect` behind the File tool and no parameters to look up: the facts come
/// off the snapshot and the two settings are the session's, not the document's.
/// They are not undoable for that reason — how the next export is written is
/// not part of the picture's history.
public struct FilePanel: View {
    let store: SessionStore

    public init(store: SessionStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            InspectorSection(effect: "file", title: "File") {
                ForEach(facts, id: \.0) { fact in
                    infoRow(fact.0, fact.1, truncatingHead: fact.0 == "Folder")
                }
            }
            InspectorSection(effect: "file", title: "Export") {
                formatRow
                qualityRow
            }
        }
        .padding(.vertical, 6)
    }

    // ---- what the photograph is ------------------------------------------

    /// The five rows of `file_page`, in its order.
    ///
    /// Source and Output are both here because they differ: the crop decides
    /// how much picture there is, a quarter-turn swaps the axes, and the resize
    /// decides how many pixels it is delivered in. The engine works the second
    /// number out — see `Snapshot.outputWidth` — so the two cannot disagree
    /// with what an export actually writes.
    private var facts: [(String, String)] {
        let snap = store.snapshot
        let path = snap.path.map { URL(fileURLWithPath: $0) }
        return [
            ("Name", snap.name ?? "test chart"),
            ("Folder", path?.deletingLastPathComponent().path ?? ""),
            ("Source", "\(snap.width) × \(snap.height)"),
            ("Output", "\(snap.outputWidth) × \(snap.outputHeight)"),
            ("In the set", inTheSet),
        ]
    }

    /// Where this photograph sits in the open set.
    ///
    /// `of 1` rather than `of 0` when a single photograph was opened on its
    /// own: one picture is a set of one, and "1 of 0" is not a thing anybody
    /// can read.
    private var inTheSet: String {
        let library = store.library
        return "\((library.current ?? 0) + 1) of \(max(library.count, 1))"
    }

    // ---- what it will be written as --------------------------------------

    /// The format, as the menu every other choice in this shell uses.
    ///
    /// The Windows shell draws three chips side by side; this is the same
    /// choice in this shell's own idiom, which is what `ChoiceRow` and
    /// `CropPanel`'s aspect lock both already look like.
    private var formatRow: some View {
        HStack(spacing: RowMetrics.gap) {
            label("Format")
            ChoiceMenu(
                options: ExportFormat.all.map(\.label),
                chosen: ExportFormat.label(of: store.snapshot.exportFormat)
            ) { chosen in
                // The quality goes with it. `setExport` takes both, so sending
                // a format alone would quietly reset whatever quality had been
                // chosen — and switching to a PNG and back would be a silent
                // way to lose it.
                store.setExport(
                    format: ExportFormat.name(ofLabel: chosen),
                    quality: store.snapshot.exportQuality
                )
            }
            .frame(maxWidth: 132)
            Spacer(minLength: 0)
        }
        .frame(height: RowMetrics.height)
    }

    /// The quality, greyed rather than hidden for a PNG.
    ///
    /// A control that vanishes takes its explanation with it. The row staying
    /// put, dimmed, says "quality is a JPEG idea" far better than an empty
    /// space does — and `ScalarRow` both dims and disables from the one flag,
    /// so a drag on a PNG's quality does nothing as well as looking like it.
    ///
    /// Which formats carry a quality is the engine's answer, not this shell's:
    /// see `Format::takes_quality`.
    private var qualityRow: some View {
        ScalarRow(
            name: "Quality",
            unit: "",
            value: Float(store.snapshot.exportQuality),
            // 95 rather than 100 for the reason `export.rs` gives: the last few
            // points of a JPEG scale buy almost nothing you can see and cost a
            // great deal of file, and 100 is still lossy.
            bounds: Bounds(min: 1, max: 100, default: 95, neutral: 95),
            isActive: ExportFormat.takesQuality(name: store.snapshot.exportFormat),
            onChange: { quality in
                store.setExport(
                    format: store.snapshot.exportFormat,
                    quality: UInt8(clamping: Int(quality.rounded()))
                )
            },
            // No interaction pair, and none wanted. `beginInteraction` opens an
            // undo step; how the next export is written is a session setting
            // and not part of the picture's history.
            onBegin: {},
            onEnd: {}
        )
    }

    // ---- drawing ----------------------------------------------------------

    private func infoRow(_ name: String, _ value: String, truncatingHead: Bool)
        -> some View
    {
        HStack(spacing: RowMetrics.gap) {
            label(name)
            Text(value)
                .font(.system(size: 11.5))
                .foregroundStyle(Palette.title.color)
                .lineLimit(1)
                // A folder is long and its *last* component is the one worth
                // reading, so it loses its front. A name loses its tail, which
                // is usually the extension.
                .truncationMode(truncatingHead ? .head : .tail)
                .help(value)
            Spacer(minLength: 0)
        }
        .frame(height: RowMetrics.height)
    }

    /// The label column, drawn exactly as ``ScalarRow`` draws its own, so the
    /// rows line up with the quality slider below them.
    private func label(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 11.5))
            .foregroundStyle(Palette.label.color)
            .frame(width: RowMetrics.label, alignment: .trailing)
            .lineLimit(1)
            .truncationMode(.tail)
    }
}
