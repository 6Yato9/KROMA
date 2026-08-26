import SwiftUI

struct ContentView: View {
    let store: SessionStore

    /// Whether the scopes panel is open. Off to begin with: it is a full extra
    /// render and a 1.2 MB readback per edit, and somebody who has not asked
    /// for it should not be paying for it.
    @AppStorage("showScopes") private var showScopes = false

    /// Which tool the inspector is showing, remembered between launches — the
    /// panel you were grading in is the panel you want back.
    ///
    /// Stored as the name rather than as a `Tool`, and resolved here, so a
    /// preference written by an older build that named a tool this one no
    /// longer has opens on Basic instead of on nothing.
    @AppStorage("tool") private var toolName = Tool.basic.rawValue

    private var tool: Tool { Tool(rawValue: toolName) ?? .basic }

    private var chosenTool: Binding<Tool> {
        Binding(get: { tool }, set: { toolName = $0.rawValue })
    }

    var body: some View {
        HSplitView {
            VStack(spacing: 0) {
                viewerAndScopes
                statusBar
            }
            inspector
                // Wide enough for a row, and resizable. It was pinned at 260,
                // which is less than the label, readout and reset arrow cost
                // between them — every control in the application was drawing
                // its label with the front clipped off.
                .frame(
                    minWidth: RowMetrics.minimumPanel,
                    idealWidth: 330,
                    maxWidth: 520
                )
        }
        .frame(minWidth: 900, minHeight: 560)
        // Behind the splits, so the seams between them are the panel grey
        // rather than whatever the window's own background happens to be.
        .background(Palette.panel.color)
        .onAppear {
            store.setSupportDirectory(Self.supportDirectory)
            store.openTestChart()
        }
    }

    /// The photograph, with the scopes under it when they are asked for.
    ///
    /// A split rather than a fixed height, because how much of the window a
    /// colourist gives the scopes is the sort of thing they change per
    /// photograph — and it is how the inspector is already divided from the
    /// picture.
    @ViewBuilder
    private var viewerAndScopes: some View {
        if showScopes {
            VSplitView {
                viewer
                    .frame(minWidth: 480, minHeight: 200)
                ScopesPanel(store: store)
                    .frame(minHeight: 140, idealHeight: 240, maxHeight: .infinity)
                    .background(Palette.panel.color)
            }
        } else {
            viewer
                .frame(minWidth: 480, minHeight: 320)
        }
    }

    /// The photograph, on the darkest of the four greys.
    ///
    /// `VIEWER` and not `PANEL`, and the difference is not taste: a surround
    /// lighter than the picture's own shadows makes the shadows look lifted,
    /// which is a lie told to the one person in the room grading them.
    private var viewer: some View {
        MetalViewer(store: store)
            .background(Palette.viewer.color)
    }

    /// The strip, and under it the one tool it has chosen.
    ///
    /// Eleven pinned panels used to stack in this one column, which meant
    /// reaching the warper by scrolling past a hundred and thirty controls.
    /// Now the strip is the header and the column below it is whichever tool
    /// is showing — the panels still fold, because folding is what makes one
    /// effect's thirty parameters navigable and the strip is about the other
    /// ten effects entirely.
    private var inspector: some View {
        VStack(spacing: 0) {
            ToolStrip(chosen: chosenTool)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if tool == .effects {
                        addedRows
                        // The browser belongs to this tool now rather than
                        // sitting under every panel, and it is directly under
                        // the rows it adds to — so an added effect appears
                        // where the reader is already looking.
                        EffectBrowser(registry: store.registry, store: store)
                            .padding(.vertical, 8)
                    } else {
                        pinnedPanels
                    }
                }
                .padding(.horizontal, RowMetrics.inset)
            }
        }
        .background(Palette.panel.color)
    }

    /// The chosen tool's fixed panels, in the order the tool lists them.
    ///
    /// `Tool.draws` is what decides, here and in `ToolStripTests` both, so the
    /// property the test asserts — every row of the document is drawn by
    /// exactly one of the six tools — is a property of what this view actually
    /// draws rather than of a second copy of the rule.
    @ViewBuilder
    private var pinnedPanels: some View {
        ForEach(tool.draws(store.snapshot.rows)) { drawn in
            if let effect = store.registry.effect(drawn.row.effect) {
                InspectorPanel(
                    effect: effect,
                    row: drawn.row,
                    store: store,
                    // Accented only where the effect *is* the tool. Basic draws
                    // six of these and six accented names is no more use than
                    // none.
                    namesTheTool: tool.effects.count == 1
                )
                Hairline()
            }
        }
    }

    /// Everything the user added, as the stack rows they are.
    ///
    /// `drawn.index` is the row's place in the whole document, not in this
    /// list: the reorder arrows move rows within the stack, and numbering a
    /// filtered list from zero would move the wrong one.
    @ViewBuilder
    private var addedRows: some View {
        ForEach(Tool.effects.draws(store.snapshot.rows)) { drawn in
            if let effect = store.registry.effect(drawn.row.effect) {
                StackRowView(
                    effect: effect,
                    row: drawn.row,
                    index: drawn.index,
                    count: store.snapshot.rows.count,
                    floor: store.snapshot.rows.filter(\.pinned).count,
                    store: store
                )
                // An added row's name is already drawn by the header above it,
                // beside the box that bypasses it.
                InspectorPanel(
                    effect: effect, row: drawn.row, store: store, showsTitle: false)
                Hairline()
            }
        }
    }

    /// `~/Library/Application Support/Kroma`, which is where a Mac application
    /// keeps what belongs to it. The engine does not guess this; it is told.
    static var supportDirectory: URL {
        let base = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("Kroma", isDirectory: true)
    }

    /// The passes counter, which is the number worth watching: with a deep
    /// stack, dragging the deepest slider should read 1.
    private var statusBar: some View {
        HStack(spacing: 12) {
            if let problem = store.problem {
                Text(problem)
                    .foregroundStyle(Palette.error.color)
                    .lineLimit(1)
                    .help(problem)
            } else if store.snapshot.isOpen {
                Text(store.snapshot.name ?? "test chart")
                    .foregroundStyle(Palette.label.color)
                Text("\(store.snapshot.width)x\(store.snapshot.height)")
                    .foregroundStyle(Palette.dim.color)
            }
            Spacer()
            Toggle("Scopes", isOn: $showScopes)
                .toggleStyle(KromaToggleButtonStyle())
                .help("Waveform, parade, vectorscope and histogram")
            Text("passes \(store.snapshot.passes)")
                .foregroundStyle(Palette.label.color)
                .monospacedDigit()
        }
        .font(.system(size: 11))
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        // The status bar is a panel, like the inspector and the scopes. It was
        // `.bar` — a system material — which is how one background became
        // three different greys on one screen.
        .background(Palette.panel.color)
        .overlay(alignment: .top) { Hairline() }
    }
}
