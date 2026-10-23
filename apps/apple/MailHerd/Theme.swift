import AppKit
import SwiftUI

extension Color {
    /// "#rrggbb" from the core. Grey if unreadable.
    init(hex: String) {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        let value = UInt32(digits, radix: 16) ?? 0x8a9099
        self.init(
            red: Double((value >> 16) & 0xff) / 255,
            green: Double((value >> 8) & 0xff) / 255,
            blue: Double(value & 0xff) / 255
        )
    }
}

/// The colours the Linux app uses, adapted to light and dark mode.
enum Theme {
    static let accent = Color(hex: "#2f6fdb")
    static let bubbleOther = Color(nsColor: NSColor(name: nil) { appearance in
        appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            ? NSColor(red: 0.149, green: 0.165, blue: 0.2, alpha: 1)
            : NSColor(red: 0.925, green: 0.933, blue: 0.949, alpha: 1)
    })
    static let card = Color(nsColor: .textBackgroundColor)
    static let danger = Color(hex: "#d93025")
}

/// A round avatar with initials.
struct AvatarView: View {
    let avatar: Avatar
    var size: CGFloat = 40

    var body: some View {
        Circle()
            .fill(Color(hex: avatar.color))
            .frame(width: size, height: size)
            .overlay(
                Text(avatar.initials)
                    .font(.system(size: size * 0.38, weight: .semibold))
                    .foregroundStyle(.white)
            )
            .accessibilityHidden(true)
    }
}

/// The small coloured dots that show which accounts hold a message.
struct AccountDots: View {
    let colors: [String]

    var body: some View {
        HStack(spacing: 3) {
            ForEach(colors, id: \.self) { color in
                Circle().fill(Color(hex: color)).frame(width: 6, height: 6)
            }
        }
        .accessibilityHidden(true)
    }
}
