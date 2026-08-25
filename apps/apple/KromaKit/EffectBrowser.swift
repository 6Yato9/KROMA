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

    @State private var hovering = false

    public var body: some View {
        Menu {
            // Skipping the empty ones. Every Basic effect is pinned, so that
            // heading would name a group you can add nothing from — which
            // reads as a bug rather than as a fact about the colour page.
            ForEach(registry.groups.filter { !addable(in: $0).isEmpty }, id: \.self) { group in
                Section(group) {
                    ForEach(addable(in: group)) { effect in
                        Button(effect.name) {
                            store.addEffect(effect.key)
                        }
                    }
                }
            }
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
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .onHover { hovering = $0 }
        .opacity(store.snapshot.isOpen ? 1 : ScalarRow.dimmed)
        .disabled(!store.snapshot.isOpen)
    }

    /// Everything in a group except the pinned rows, which are already in every
    /// document and would do nothing useful twice.
    private func addable(in group: String) -> [Effect] {
        registry.effects.filter { $0.group == group && !registry.pinned.contains($0.key) }
    }
}
