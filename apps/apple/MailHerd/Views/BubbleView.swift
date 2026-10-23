import AppKit
import SwiftUI

/// One message as a chat bubble (or a card, for newsletters).
struct BubbleView: View {
    @EnvironmentObject var model: AppModel
    let bubble: Bubble
    let isGroup: Bool
    @State private var expanded = false
    @State private var showOriginal = false

    private var mine: Bool { bubble.isMe }
    private var onAccent: Bool { mine && !bubble.isCard }
    private var ink: Color { onAccent ? .white : .primary }
    private var softInk: Color { onAccent ? Color(hex: "#dbe6fb") : .secondary }

    private var status: String {
        if bubble.sending { return "Sending…" }
        if bubble.failed { return "Not sent, will retry" }
        return bubble.timeLabel
    }

    var body: some View {
        VStack(alignment: mine ? .trailing : .leading, spacing: 3) {
            if let subject = bubble.subjectChange {
                Text("Subject: \(subject)")
                    .font(.caption.italic())
                    .foregroundStyle(.tertiary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 4)
            }
            if bubble.showSender && !mine {
                Text(bubble.senderName)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.leading, isGroup ? 44 : 4)
            }
            HStack(alignment: .bottom, spacing: 8) {
                if mine { Spacer(minLength: 60) }
                if isGroup && !mine {
                    if bubble.showSender {
                        AvatarView(avatar: bubble.avatar, size: 32)
                    } else {
                        Color.clear.frame(width: 32, height: 1)
                    }
                }
                content
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(
                        RoundedRectangle(cornerRadius: 16)
                            .fill(bubble.isCard ? Theme.card : (mine ? Theme.accent : Theme.bubbleOther))
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 16)
                            .stroke(bubble.isCard ? Color.secondary.opacity(0.3) : .clear)
                    )
                    .opacity(bubble.sending ? 0.75 : 1)
                    .frame(maxWidth: bubble.isCard ? 640 : 520, alignment: mine ? .trailing : .leading)
                if !mine { Spacer(minLength: 60) }
            }
            HStack(spacing: 4) {
                Text(status)
                    .font(.caption2)
                    .foregroundStyle(bubble.failed ? Theme.danger : .secondary)
                AccountDots(colors: bubble.accountColors)
            }
            .padding(.horizontal, isGroup && !mine ? 46 : 6)
        }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 6) {
            if bubble.isCard {
                Text(bubble.senderName).font(.callout.bold()).foregroundStyle(ink)
            }
            if !bubble.text.isEmpty {
                Text(bubble.text)
                    .foregroundStyle(ink)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if bubble.forwarded {
                Text("Forwarded").font(.caption2).foregroundStyle(softInk)
            }
            if bubble.hasHidden {
                Button(expanded ? "Hide" : "•••") { expanded.toggle() }
                    .buttonStyle(.plain)
                    .font(.caption)
                    .foregroundStyle(ink)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 2)
                    .background(Capsule().fill(onAccent ? Color(hex: "#3d7ce3") : Color.secondary.opacity(0.2)))
                    .accessibilityLabel(expanded ? "Hide quoted text" : "Show quoted text")
                if expanded {
                    Text([bubble.signature, bubble.quoted].compactMap { $0 }.joined(separator: "\n\n"))
                        .font(.callout)
                        .foregroundStyle(softInk)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if bubble.hasHtml && bubble.isCard {
                Button(showOriginal ? "Hide full email" : "Show full email") { showOriginal.toggle() }
                    .buttonStyle(.link)
                    .font(.callout)
                if showOriginal {
                    HTMLView(html: model.html(bubble.messageId))
                        .frame(minHeight: 320)
                }
            }
            ForEach(bubble.attachments, id: \.self) { attachment in
                Label("\(attachment.filename) · \(attachment.sizeLabel)", systemImage: "paperclip")
                    .font(.caption)
                    .foregroundStyle(ink)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(
                        RoundedRectangle(cornerRadius: 8)
                            .fill(onAccent ? Color(hex: "#3d7ce3") : Color.secondary.opacity(0.12))
                    )
                    .accessibilityLabel("Attachment \(attachment.filename), \(attachment.sizeLabel)")
            }
        }
    }
}
