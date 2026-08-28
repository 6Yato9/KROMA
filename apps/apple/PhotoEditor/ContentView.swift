import SwiftUI

struct ContentView: View {
    let store: SessionStore

    /// Whether the scopes panel is open. Off to begin with: it is a full extra
    /// render and a 1.2 MB readback per edit, and somebody who has not asked
    /// for it should not be paying for it.
    @AppStorage("showScopes") private var showScopes = false

    /// Which tab the inspector is showing, remembered between launches — the
    /// page you were grading in is the page you want back.
    ///
    /// Stored as the name rather than as a `Tab`, and resolved here, so a
    /// preference written by an older build naming something this one no longer
    /// has opens on Colour instead of on nothing.
    ///
    /// A *new* key, and deliberately: `"tool"` holds names like
    /// `"Colour Wheels"`, which are sections of the Colour tab now rather than
    /// pages of their own. Reusing the key would open every existing install on
    /// a fallback instead of on what it was left showing.
    @AppStorage("tab") private var tabName = Tab.colour.rawValue

    private var tab: Tab { Tab(rawValue: tabName) ?? .colour }

    private var chosenTab: Binding<Tab> {
        Binding(get: { tab }, set: { tabName = $0.rawValue })
    }

    var body: some View {
        VStack(spacing: 0) {
            // Across the whole window, above the filmstrip and the split, which
            // is where `main.rs` puts its `TopBottomPanel::top`. These are
            // properties of the *window* — how the picture is being looked at —
            // and none of them belongs to the viewer pane alone.
            //
            // They were in the status bar, which is only as wide as that pane
            // because the inspector sits beside it. Eight controls and two
            // readouts in a third of the window truncated "Compare" to "Co…"
            // and wrapped "passes 0" onto two lines.
            toolbar
            content
        }
        .frame(minWidth: 900, minHeight: 560)
        // Behind the splits, so the seams between them are the panel grey
        // rather than whatever the window's own background happens to be.
        .background(Palette.panel.color)
        .onAppear {
            store.setSupportDirectory(Self.supportDirectory)
            // Before this line, and it matters: the engine reads the settings
            // file as part of being told where the support directory is, so
            // there is nothing to reopen until it has been.
            //
            // The chart is the fallback rather than the default. A first run
            // and a set whose files have all gone both land on it, and so does
            // a set none of whose photographs will decode — see
            // `SessionStore.openRemembered`, which is where the whole of that
            // policy is written down and where a refusal is turned into
            // something the status bar can say.
            store.openRemembered()
            // The tab is remembered between launches, so this may already be
            // Image — in which case the viewer has to open on the enclosing
            // frame rather than waiting for a change that never comes.
            store.setCropping(tab.showsWholeFrame)
        }
        // The engine frames the viewer on the whole straightened source while
        // the Image tab is showing, so there is something outside the rectangle
        // to see and to drag back into — and, just as importantly, it is turned
        // off again on the way out. Left on, every other tab would be grading
        // a picture with the cut-away parts still in it.
        //
        // Driven from here rather than from `CropOverlay`'s `onAppear`, because
        // the flag decides what the *viewer* draws and the overlay is only what
        // goes on top of it.
        .onChange(of: tab) { _, chosen in
            store.setCropping(chosen.showsWholeFrame)
        }
    }

    private var content: some View {
        HStack(spacing: 0) {
            // Down the left, not across the bottom. The window is wider than
            // it is tall and a photograph is not, so a horizontal strip costs
            // height — the dimension the picture is already short of.
            //
            // Beside the split rather than inside it: a filmstrip's width is
            // the width of a thumbnail and there is nothing for a wider one to
            // show, so there is no reason to offer a handle that only makes it
            // wrong. And unconditional, because the strip draws nothing at all
            // for a set of one or none — which set gets a strip is decided in
            // `Filmstrip` and nowhere else.
            Filmstrip(store: store)
            HSplitView {
                VStack(spacing: 0) {
                    viewerAndScopes
                    statusBar
                }
                inspector
                    // Wide enough for a row, and resizable. It was pinned at
                    // 260, which is less than the label, readout and reset
                    // arrow cost between them — every control in the
                    // application was drawing its label with the front clipped
                    // off.
                    .frame(
                        minWidth: RowMetrics.minimumPanel,
                        idealWidth: 330,
                        maxWidth: 520
                    )
            }
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
    ///
    /// The overlays go over it rather than inside `MetalViewer`: the viewer
    /// hands a layer to the engine and every pixel in it is drawn by Rust, so a
    /// SwiftUI rectangle cannot live there. Over the top they are ordinary
    /// views — but only drawing ones. Neither takes the pointer; the drags they
    /// belong to are `MetalViewerView`'s, along with the zoom and the pan, for
    /// the reason `ViewerDrag` sets out.
    ///
    /// The comparison is unconditional and the crop overlay is not, because the
    /// comparison is a property of the window that survives changing tabs
    /// while the crop rectangle is the Image tab's own. Off, the comparison
    /// draws nothing at all.
    private var viewer: some View {
        MetalViewer(store: store)
            .background(Palette.viewer.color)
            .overlay {
                ZStack {
                    if tab.showsWholeFrame { CropOverlay(store: store) }
                    CompareOverlay(store: store)
                }
            }
            // An effect dropped on the picture is added, which is `main.rs`'s
            // "the picture is the larger target — which matters when the thing
            // you are deciding about is what the picture will look like".
            //
            // A dragging destination, not a mouse handler: the wheel, the pan
            // and the double-click still belong to `MetalViewerView`. That
            // separation is the thing to check by hand — the crop overlay
            // silently took all three once.
            .dropDestination(for: DraggedEffect.self) { dropped, _ in
                dropped.forEach { store.addEffect($0.key) }
                return !dropped.isEmpty
            }
    }

    /// The header, the tab row, and under them the one tab that is chosen.
    ///
    /// Eleven pinned panels used to stack in this one column, which meant
    /// reaching the warper by scrolling past a hundred and thirty controls.
    /// `main.rs`'s answer, and now this one: four tabs, with the whole grade
    /// under the first of them divided into five collapsing sections. The
    /// panels inside a section still fold, because folding is what makes one
    /// effect's thirty parameters navigable and the sections are about the
    /// other ten effects entirely.
    private var inspector: some View {
        VStack(spacing: 0) {
            InspectorHeader(store: store)
            TabRow(chosen: chosenTab)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch tab {
                    case .colour:
                        colourSections
                    case .effects:
                        // The whole list is the target rather than the gap
                        // between two rows: choosing a position on the way in
                        // would need an insertion indicator and a
                        // scroll-while-dragging story, and the reorder arrows
                        // already move a row once it is there. `take_drop` in
                        // `inspector.rs` says the same.
                        //
                        // The floor is so there is something to drop on before
                        // the first effect is added: an empty list has no
                        // height, and a target with no area is not one.
                        VStack(alignment: .leading, spacing: 0) { addedRows }
                            .frame(
                                maxWidth: .infinity, minHeight: 44,
                                alignment: .topLeading
                            )
                            .contentShape(Rectangle())
                            .dropDestination(for: DraggedEffect.self) { dropped, _ in
                                dropped.forEach { store.addEffect($0.key) }
                                return !dropped.isEmpty
                            }
                        // The browser is directly under the rows it adds to, so
                        // an added effect appears where the reader is already
                        // looking. `main.rs` puts its shelf above the list; the
                        // order is this shell's own and the reason is stated.
                        EffectBrowser(registry: store.registry, store: store)
                            .padding(.vertical, 8)
                    case .image:
                        // The one tab with no rows behind it: it edits the
                        // document's geometry, which is a value on the document
                        // rather than an entry in its stack.
                        CropPanel(store: store)
                    case .file:
                        // Also no rows behind it: this is the file, and the two
                        // settings the next export will be written with,
                        // neither of which is an entry in the document's stack.
                        FilePanel(store: store)
                    }
                }
                .padding(.horizontal, RowMetrics.inset)
            }
        }
        .background(Palette.panel.color)
    }

    /// The Colour tab: five collapsing sections, in the engine's order.
    ///
    /// `Section.draws` is what decides, here and in `TabRowTests` both, so the
    /// property the test asserts — every row of the document is drawn by
    /// exactly one of the five sections or by the Effects tab — is a property
    /// of what this view actually draws rather than of a second copy of the
    /// rule.
    ///
    /// **Nothing here is accented.** The section heading already names the
    /// effect, so accenting the panel's own title would be the same word twice
    /// in the same colour — and every heading accented at once says exactly as
    /// little as none of them being.
    @ViewBuilder
    private var colourSections: some View {
        ForEach(Section.allCases, id: \.self) { section in
            InspectorSection(
                // Keyed by the tab as well as the section, so a section folded
                // here says nothing about a parameter group of the same name
                // inside some effect.
                effect: "colour",
                title: section.title,
                startsOpen: section.startsOpen
            ) {
                ForEach(section.draws(store.snapshot.rows)) { drawn in
                    if let effect = store.registry.effect(drawn.row.effect) {
                        InspectorPanel(
                            effect: effect,
                            row: drawn.row,
                            store: store,
                            // A section that *is* one effect has already said
                            // its name: "Curves" inside "Curves" is the same
                            // word twice with a fold between them. `main.rs`
                            // does the same — its one-effect headers open
                            // straight onto the controls.
                            showsTitle: section.effects.count > 1
                        )
                        Hairline()
                    }
                }
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
        ForEach(Tab.added(store.snapshot.rows)) { drawn in
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

    /// What the zoom is worth, as a percentage.
    ///
    /// An em dash rather than "100%" when there is nothing to measure: with no
    /// layer attached the honest answer is that there is no answer, and a
    /// plausible number would be read as one.
    private var zoomReadout: String {
        guard let scale = store.viewScale else { return "—" }
        return "\(Int((scale * 100).rounded()))%"
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
    /// How the picture is being looked at, across the whole window.
    ///
    /// `main.rs`'s toolbar, less the parts macOS puts in the menu bar: File,
    /// Export, Grade and undo are menus there because that is where a Mac user
    /// looks for them, and duplicating them here would be two ways to do one
    /// thing.
    ///
    /// What is left is the window's own state — which comparison is running,
    /// whether the scopes are up, how far in the picture is — and it belongs
    /// across the top rather than in the status bar, which is only as wide as
    /// the viewer pane.
    private var toolbar: some View {
        HStack(spacing: 8) {
            // Beside the scopes, because both are ways of looking at the
            // photograph rather than things done to it.
            CompareButton(store: store)
            Toggle("Scopes", isOn: $showScopes)
                .toggleStyle(KromaToggleButtonStyle())
                .help("Waveform, parade, vectorscope and histogram")

            Divider().frame(height: 14)

            // Fit, 100%, and what the zoom is worth — `main.rs`'s three, in its
            // order. Fit is greyed when the whole picture is already on screen,
            // because there is nothing for it to do.
            Button("Fit") { store.fitView() }
                .buttonStyle(KromaButtonStyle())
                .disabled(store.isFit)
                .help("Double-click the picture")
            Button("100%") { store.zoomToActualPixels() }
                .buttonStyle(KromaButtonStyle())
                // Greyed by what is *open*, not by whether a scale can be
                // measured. `viewScale` reaches through to the engine, and
                // `@Observable` cannot see through that — a button bound to it
                // would keep whatever state it was built with, which before
                // the first frame is disabled and would stay disabled.
                .disabled(!store.snapshot.isOpen)
                .help("One image pixel to one screen pixel")
            Text(zoomReadout)
                .foregroundStyle(Palette.label.color)
                .monospacedDigit()
                // A fixed width, so the row does not shuffle sideways every
                // time the number gains or loses a digit mid-drag.
                .frame(width: 44, alignment: .trailing)
                .help("Screen pixels per image pixel")

            Divider().frame(height: 14)

            Text("passes \(store.snapshot.passes)")
                .foregroundStyle(Palette.label.color)
                .monospacedDigit()
                .fixedSize()
                .help(
                    "GPU passes executed this frame. Dragging one slider in a deep stack "
                        + "should read 1 — that is the stage cache doing its job.")

            Spacer(minLength: 8)

            // Last and quietly, as `main.rs` has it. Nobody reads this until
            // something has gone wrong, and then it is the first thing worth
            // knowing — which is why it is present rather than in an About box
            // nobody opens. Absent until the first frame: the engine will not
            // acquire a device just to be named.
            //
            // Shown whole or not at all. Told to truncate it drew a bare "…" in
            // a narrow window, which is worse than the space: an ellipsis says
            // something is there and refuses to say what.
            if let gpu = store.gpuName {
                ViewThatFits(in: .horizontal) {
                    Text(gpu)
                        .font(.system(size: 10))
                        .foregroundStyle(Palette.dim.color)
                        .lineLimit(1)
                        .fixedSize()
                    Color.clear.frame(width: 0, height: 0)
                }
                .help(gpu)
            }
        }
        .font(.system(size: 11))
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .frame(maxWidth: .infinity)
        .background(Palette.raised.color)
        .overlay(alignment: .bottom) { Hairline() }
    }

    /// What just happened, or what is open.
    ///
    /// One line and nothing else, which is all `main.rs` puts here. Everything
    /// that used to share it now lives in the toolbar: this bar is as wide as
    /// the viewer pane, and eight controls in that width truncated their own
    /// labels.
    ///
    /// Always present rather than appearing with the first message. A bar that
    /// comes and goes moves the photograph up and down under it, and a
    /// photograph that jumps while you are judging it is worse than a strip of
    /// window spent on the name of what you are looking at.
    private var statusBar: some View {
        HStack(spacing: 6) {
            if let problem = store.problem {
                Text(problem)
                    .foregroundStyle(Palette.error.color)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(problem)
            } else if let notice = store.notice {
                Text(notice)
                    .foregroundStyle(Palette.label.color)
                    .lineLimit(1)
            } else if store.snapshot.isOpen {
                // The idle line is the name and the size, the way `main.rs`
                // writes it. Also in the inspector's header, and the two answer
                // different questions: that one says what the column of
                // controls belongs to, this says what the window is showing.
                Text(idle)
                    .foregroundStyle(Palette.dim.color)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(idle)
            } else {
                Text("no photograph open")
                    .foregroundStyle(Palette.dim.color)
            }
            Spacer(minLength: 0)
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

    /// `main.rs`'s idle line: `name — WxH`, one string so it truncates as one.
    private var idle: String {
        let name = store.snapshot.name ?? "test chart"
        return "\(name) — \(store.snapshot.width)x\(store.snapshot.height)"
    }
}
