import SwiftUI

/// The shelf that adds a row, grouped as the registry groups them, with
/// whatever has been starred in a group of its own at the top.
///
/// Nothing here lists an effect by name. `Group::ALL` exists in Rust so that
/// adding a variant and forgetting to list it is a compile error rather than an
/// effect that is fully implemented, has a shader, passes its tests — and
/// cannot be added to a stack, because nothing draws a heading for it. The same
/// property holds on this side by generating the shelf from `registry.groups`.
///
/// A shelf and not a menu, since the star arrived. A menu item is one target:
/// everywhere you can press is "add this", and there is nowhere to put a
/// second gesture that means "keep this at the top and do not add it". The
/// Windows shell reached the same conclusion — `apps/windows/src/inspector.rs`
/// gives the star its own corner of the tile and checks it *before* the tile's
/// own click, or starring an effect would also add it.
public struct EffectBrowser: View {
    let registry: Registry
    let store: SessionStore

    public init(registry: Registry, store: SessionStore) {
        self.registry = registry
        self.store = store
    }

    @State private var hovering = false
    @State private var showing = false

    public var body: some View {
        Button {
            showing.toggle()
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "plus")
                    .imageScale(.small)
                    .foregroundStyle(Palette.icon.color)
                Text("Add effect")
                    .font(.system(size: 11))
                    .foregroundStyle(Palette.label.color)
            }
            .modifier(ControlFace(hot: hovering))
        }
        .buttonStyle(.plain)
        .fixedSize()
        .onHover { hovering = $0 }
        .opacity(store.snapshot.isOpen ? 1 : ScalarRow.dimmed)
        .disabled(!store.snapshot.isOpen)
        .popover(isPresented: $showing) {
            EffectShelf(
                sections: Self.sections(in: registry, starred: store.favourites),
                isStarred: store.isFavourite,
                add: { key in
                    store.addEffect(key)
                    // The shelf is a list of things to do once. Leaving it open
                    // after one of them has been done means the next click
                    // lands on a panel that is no longer the one in front.
                    showing = false
                },
                star: store.toggleFavourite
            )
        }
    }

    /// What the shelf shows, in the order it shows it.
    ///
    /// A function of the registry and the stars and nothing else, so that the
    /// rule can be checked without drawing anything.
    ///
    /// Three rules, all of them `inspector.rs`'s:
    ///
    /// - **Starred first, and only there.** An effect listed twice would be two
    ///   tiles that do the same thing, one of them wrong the moment the star on
    ///   the other is clicked.
    /// - **The pinned rows are not on offer**, in either group. They are in
    ///   every document already and adding one would do nothing useful twice.
    /// - **A heading with nothing under it is not drawn.** Every Basic effect
    ///   is pinned, so that heading would name a group you can add nothing
    ///   from — which reads as a list that failed to load. So does a group
    ///   whose entries have all been starred away into the one above.
    static func sections(in registry: Registry, starred: [String]) -> [EffectSection] {
        let kept = Set(starred)
        let addable = registry.effects.filter { !registry.pinned.contains($0.key) }

        var sections: [EffectSection] = []
        let favourites = addable.filter { kept.contains($0.key) }
        if !favourites.isEmpty {
            sections.append(EffectSection(heading: "Favourites", effects: favourites))
        }
        for group in registry.groups {
            let rest = addable.filter { $0.group == group && !kept.contains($0.key) }
            if rest.isEmpty { continue }
            sections.append(EffectSection(heading: group, effects: rest))
        }
        return sections
    }
}

/// One heading and the effects under it.
struct EffectSection: Identifiable {
    let heading: String
    let effects: [Effect]

    var id: String { heading }
}

/// The shelf, with the scroll it needs when everything is starred and nothing
/// is.
///
/// The tiles are a view of their own for the reason `FilmstripCells` is:
/// `ImageRenderer` draws nothing inside a `ScrollView`, because a scroll view
/// has no viewport off screen and never lays its content out. `EffectShelfList`
/// is the largest piece of this a headless test can be pointed at, and it is
/// the piece the tiles and their stars live in.
struct EffectShelf: View {
    let sections: [EffectSection]
    let isStarred: (String) -> Bool
    let add: (String) -> Void
    let star: (String) -> Void

    var body: some View {
        ScrollView {
            EffectShelfList(sections: sections, isStarred: isStarred, add: add, star: star)
                .padding(EffectShelfList.inset)
        }
        .frame(width: EffectShelfList.width, height: EffectShelfList.height)
        .background(Palette.panel.color)
    }
}

/// The headings and the tiles under them.
struct EffectShelfList: View {
    let sections: [EffectSection]
    let isStarred: (String) -> Bool
    let add: (String) -> Void
    let star: (String) -> Void

    /// How wide the shelf is, and how much of it is on screen before it
    /// scrolls. The Windows shelf's `BROWSER_HEIGHT`, in points.
    static let width: CGFloat = 240
    static let height: CGFloat = 420
    static let inset: CGFloat = 6

    /// One tile, and the gap under it.
    static let tileHeight: CGFloat = 24
    static let gap: CGFloat = 3
    static var stride: CGFloat { tileHeight + gap }

    /// The star's own corner: its centre, measured in from the tile's right
    /// edge, and how big a target it is.
    static let starInset: CGFloat = 12
    static let starTarget: CGFloat = 20
    /// The outer radius of the star itself, which is `inspector.rs`'s.
    static let starRadius: CGFloat = 6

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(sections) { section in
                Text(section.heading)
                    .font(.system(size: 10))
                    .foregroundStyle(Palette.label.color)
                    .padding(.top, 4)
                    .padding(.bottom, 2)
                ForEach(section.effects) { effect in
                    EffectTile(
                        effect: effect,
                        starred: isStarred(effect.key),
                        add: { add(effect.key) },
                        toggle: { star(effect.key) }
                    )
                    .padding(.bottom, Self.gap)
                }
            }
        }
    }
}

/// One tile: the name, and a star in the corner that is a control of its own.
struct EffectTile: View {
    let effect: Effect
    let starred: Bool
    let add: () -> Void
    let toggle: () -> Void

    @State private var hovering = false
    @State private var overStar = false

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 4)
                .fill(hot ? Palette.control.color : Palette.panel.color)
            RoundedRectangle(cornerRadius: 4)
                .strokeBorder(
                    hot ? Palette.accent.color : Palette.controlHot.color, lineWidth: 1)
            Text(effect.name)
                .font(.system(size: 11.5))
                .foregroundStyle((hot ? Palette.title : Palette.label).color)
                .lineLimit(1)
                .padding(.leading, 8)
                // Room for the star, so a long name is truncated rather than
                // written under it.
                .padding(.trailing, EffectShelfList.starInset + EffectShelfList.starTarget / 2)
        }
        .frame(height: EffectShelfList.tileHeight)
        .contentShape(Rectangle())
        .onTapGesture(perform: add)
        .onHover { hovering = $0 }
        .overlay(alignment: .trailing) {
            // Over the tile and after it, so the star's own hit area wins: a
            // press on the star must not also add the effect.
            star
                .frame(
                    width: EffectShelfList.starRadius * 2,
                    height: EffectShelfList.starRadius * 2
                )
                .frame(
                    width: EffectShelfList.starTarget, height: EffectShelfList.starTarget)
                .contentShape(Rectangle())
                .onTapGesture(perform: toggle)
                .onHover { overStar = $0 }
                .offset(
                    x: EffectShelfList.starTarget / 2 - EffectShelfList.starInset)
                .help(starred ? "\(effect.name) — starred" : "Keep \(effect.name) at the top")
        }
    }

    /// Filled when it is starred, an outline when it is not — and the outline
    /// warms under the pointer, so that the gesture is discoverable at all.
    @ViewBuilder
    private var star: some View {
        if starred {
            Star().fill(Palette.warn.color)
        } else {
            Star().stroke((overStar ? Palette.warn : Palette.dim).color, lineWidth: 1.2)
        }
    }

    /// Not while the pointer is on the star: that gesture is about the shelf,
    /// not about the effect.
    private var hot: Bool { hovering && !overStar }
}

/// A five-pointed star.
///
/// `inspector.rs`'s, point for point: ten points about a centre, alternating
/// an outer radius and an inner one at 0.44 of it, starting at the top. A
/// glyph from the system font would be a second star in an application that
/// has one, and it would be drawn in the system's own weight.
struct Star: Shape {
    func path(in rect: CGRect) -> Path {
        let outer = min(rect.width, rect.height) / 2
        let centre = CGPoint(x: rect.midX, y: rect.midY)
        let points = (0..<10).map { i -> CGPoint in
            let angle = -CGFloat.pi / 2 + CGFloat(i) * CGFloat.pi / 5
            let radius = i.isMultiple(of: 2) ? outer : outer * 0.44
            return CGPoint(
                x: centre.x + radius * cos(angle), y: centre.y + radius * sin(angle))
        }
        var path = Path()
        path.addLines(points)
        path.closeSubpath()
        return path
    }
}
