import SwiftUI

// -----------------------------------------------------------------------------
// The furniture every panel is built out of
// -----------------------------------------------------------------------------
//
// Small shapes and styles rather than a set of colours applied at each call
// site. A checkbox drawn by hand in three files is three checkboxes, and the
// one nobody looked at is the one still wearing the system's clothes.

/// Every division in the interface, drawn once.
///
/// `Divider()` is whatever the appearance the user has set says a divider is.
/// `RULE` is one number, read from `pe_theme` by both shells. A panel whose
/// separators come from the system and whose backgrounds come from Kroma is a
/// panel with two schemes in it, and the seam shows.
public struct Hairline: View {
    public init() {}

    public var body: some View {
        Rectangle()
            .fill(Palette.rule.color)
            .frame(height: 1)
    }
}

/// The chevron on a collapsible heading.
///
/// The Windows shell's, point for point — three points about a centre, open
/// pointing down and shut pointing right. A triangle glyph from the system
/// font would be a fourth typeface in a panel that has one.
public struct Chevron: Shape {
    public var open: Bool

    public init(open: Bool) {
        self.open = open
    }

    public func path(in rect: CGRect) -> Path {
        let r = min(rect.width, rect.height) / 2
        let c = CGPoint(x: rect.midX, y: rect.midY)
        let points: [CGPoint] =
            open
            ? [
                CGPoint(x: c.x - r, y: c.y - r * 0.5),
                CGPoint(x: c.x, y: c.y + r * 0.6),
                CGPoint(x: c.x + r, y: c.y - r * 0.5),
            ]
            : [
                CGPoint(x: c.x - r * 0.5, y: c.y - r),
                CGPoint(x: c.x + r * 0.6, y: c.y),
                CGPoint(x: c.x - r * 0.5, y: c.y + r),
            ]
        var path = Path()
        path.addLines(points)
        return path
    }
}

/// The tick inside a checked box.
struct Tick: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.minX, y: rect.midY + rect.height * 0.05))
        path.addLine(to: CGPoint(x: rect.minX + rect.width * 0.36, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        return path
    }
}

/// The face of anything that sits on a panel and can be pressed: a combo box,
/// a menu button, the boxed part of a control.
///
/// `CONTROL` at rest and `CONTROL_HOT` under the pointer, with a `BOX_EDGE`
/// outline — which is what `theme.rs` gives every egui widget on the other
/// side. The system's own control colour is the one thing it must not be:
/// AppKit draws that from the appearance, so a panel of them is a panel that
/// changes scheme when the user does.
public struct ControlFace: ViewModifier {
    let hot: Bool

    public init(hot: Bool) {
        self.hot = hot
    }

    public func body(content: Content) -> some View {
        content
            .padding(.horizontal, 6)
            .frame(height: ScalarRow.boxHeight)
            .background(
                RoundedRectangle(cornerRadius: 2)
                    .fill(hot ? Palette.controlHot.color : Palette.control.color)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 2)
                    .strokeBorder(Palette.boxEdge.color, lineWidth: 1)
            )
            .contentShape(Rectangle())
    }
}

/// A checkbox on the palette.
///
/// `.toggleStyle(.checkbox)` is an `NSButton`, which means the system accent
/// when it is on — the colour the user chose in System Settings, in a scheme
/// that spends its own accent on exactly one thing. This one is a `CONTROL`
/// box with a `TITLE` tick in it.
public struct KromaCheckboxStyle: ToggleStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Box(configuration: configuration)
    }

    private struct Box: View {
        let configuration: Configuration
        @State private var hovering = false

        var body: some View {
            Button {
                configuration.isOn.toggle()
            } label: {
                HStack(spacing: 5) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(hovering ? Palette.controlHot.color : Palette.control.color)
                        RoundedRectangle(cornerRadius: 2)
                            .strokeBorder(Palette.boxEdge.color, lineWidth: 1)
                        if configuration.isOn {
                            Tick()
                                .stroke(
                                    Palette.title.color,
                                    style: StrokeStyle(
                                        lineWidth: 1.6, lineCap: .round, lineJoin: .round)
                                )
                                .padding(3.5)
                        }
                    }
                    .frame(width: 13, height: 13)

                    configuration.label
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
        }
    }
}

/// A toggle drawn as a chip that stays pressed — the scopes picker, and
/// anything else that is a set of choices rather than a set of actions.
///
/// On when chosen is `SELECT`, and deliberately not `ACCENT`. "This is chosen"
/// and "this is doing something" are different facts and Resolve keeps them
/// apart: the accent titles the open effect and is spent nowhere else, so a
/// panel of accent-coloured toggles takes the one loud colour in the scheme
/// and makes it mean nothing.
public struct KromaToggleButtonStyle: ToggleStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Chip(configuration: configuration)
    }

    private struct Chip: View {
        let configuration: Configuration
        @Environment(\.isEnabled) private var enabled
        @State private var hovering = false

        var body: some View {
            Button {
                configuration.isOn.toggle()
            } label: {
                configuration.label
                    .font(.system(size: 11))
                    .foregroundStyle((configuration.isOn ? Palette.title : Palette.label).color)
                    .padding(.horizontal, 7)
                    .frame(height: ScalarRow.boxHeight)
                    .background(RoundedRectangle(cornerRadius: 2).fill(fill))
                    .overlay(
                        RoundedRectangle(cornerRadius: 2)
                            .strokeBorder(Palette.boxEdge.color, lineWidth: 1)
                    )
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            .opacity(enabled ? 1 : ScalarRow.dimmed)
        }

        private var fill: Color {
            if configuration.isOn { return Palette.select.color }
            return hovering ? Palette.controlHot.color : Palette.control.color
        }
    }
}

/// A push button on the palette. `.bordered` is the system's control colour
/// and the system's accent when it is pressed; this is `CONTROL`.
public struct KromaButtonStyle: ButtonStyle {
    public init() {}

    public func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration)
    }

    private struct Face: View {
        let configuration: Configuration
        @Environment(\.isEnabled) private var enabled
        @State private var hovering = false

        var body: some View {
            configuration.label
                .font(.system(size: 11))
                .foregroundStyle(Palette.label.color)
                .modifier(ControlFace(hot: hovering || configuration.isPressed))
                .onHover { hovering = $0 }
                .opacity(enabled ? 1 : ScalarRow.dimmed)
        }
    }
}

/// A short set of choices, side by side, with the chosen one on `SELECT`.
///
/// What `.pickerStyle(.segmented)` was doing, minus the system accent it paints
/// the chosen segment with. Short sets only: a menu is the right control past
/// about four, which is why the curve picker is one.
public struct ChoiceChips: View {
    let options: [String]
    let chosen: String
    let pick: (String) -> Void

    public init(options: [String], chosen: String, pick: @escaping (String) -> Void) {
        self.options = options
        self.chosen = chosen
        self.pick = pick
    }

    public var body: some View {
        HStack(spacing: 3) {
            ForEach(options, id: \.self) { option in
                Toggle(
                    option,
                    isOn: Binding(
                        get: { option == chosen },
                        // A chip that is already on stays on: this is a choice
                        // of one from several, not several switches.
                        set: { if $0 { pick(option) } }
                    )
                )
                .toggleStyle(KromaToggleButtonStyle())
            }
            Spacer(minLength: 0)
        }
    }
}

/// A drop-down on the palette: the current value, a chevron, and the options
/// in a menu.
///
/// A `Picker` is an `NSPopUpButton`, which cannot be recoloured — the button
/// face is the system's control colour and the chosen row is drawn in the
/// system accent. The menu that drops down is still AppKit's and still looks
/// like AppKit's; the part that sits in the panel all the time is ours.
public struct ChoiceMenu: View {
    let options: [String]
    let chosen: String
    let pick: (String) -> Void

    @State private var hovering = false

    public init(options: [String], chosen: String, pick: @escaping (String) -> Void) {
        self.options = options
        self.chosen = chosen
        self.pick = pick
    }

    public var body: some View {
        Menu {
            ForEach(options, id: \.self) { option in
                Button {
                    pick(option)
                } label: {
                    if option == chosen {
                        Label(option, systemImage: "checkmark")
                    } else {
                        Text(option)
                    }
                }
            }
        } label: {
            HStack(spacing: 4) {
                Text(chosen)
                    .font(.system(size: 11))
                    .foregroundStyle(Palette.label.color)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Chevron(open: true)
                    .stroke(
                        Palette.icon.color,
                        style: StrokeStyle(lineWidth: 1.2, lineCap: .round, lineJoin: .round)
                    )
                    .frame(width: 6, height: 6)
            }
            .modifier(ControlFace(hot: hovering))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .onHover { hovering = $0 }
    }
}

/// A bare glyph button — the row-reordering arrows, the bin.
///
/// The dim is spelled out for the reason ``ScalarRow/dimmed`` is: `.disabled`
/// fades SwiftUI's *semantic* styles, and `ICON` is not one of them, so a
/// disabled arrow would otherwise be drawn at full strength.
public struct IconButton: View {
    let symbol: String
    let help: String
    let action: () -> Void

    @Environment(\.isEnabled) private var enabled

    public init(_ symbol: String, help: String = "", action: @escaping () -> Void) {
        self.symbol = symbol
        self.help = help
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .imageScale(.small)
                .foregroundStyle(Palette.icon.color)
                .opacity(enabled ? 1 : ScalarRow.dimmed)
        }
        .buttonStyle(.borderless)
        .help(help)
    }
}
