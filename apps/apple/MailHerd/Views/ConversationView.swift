import AppKit
import SwiftUI

/// Right column: the open conversation as chat bubbles, with the reply box.
struct ConversationView: View {
    @EnvironmentObject var model: AppModel
    @State private var draft = ""

    var body: some View {
        if let conversation = model.conversation {
            VStack(spacing: 0) {
                header(conversation)
                Divider()
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 8) {
                            ForEach(conversation.items) { item in
                                switch item {
                                case .day(let label):
                                    Text(label)
                                        .font(.caption.bold())
                                        .foregroundStyle(.secondary)
                                        .padding(.horizontal, 10)
                                        .padding(.vertical, 3)
                                        .background(Capsule().fill(Theme.bubbleOther))
                                        .padding(.vertical, 6)
                                case .message(let bubble):
                                    BubbleView(bubble: bubble, isGroup: conversation.isGroup)
                                }
                            }
                            Color.clear.frame(height: 1).id("bottom")
                        }
                        .padding(16)
                    }
                    .onAppear { proxy.scrollTo("bottom") }
                    .onChange(of: conversation.items.count) { _ in
                        withAnimation { proxy.scrollTo("bottom") }
                    }
                }
                .overlay(alignment: .bottom) {
                    UndoBar(pending: model.pending.last { $0.threadId == conversation.id }
                            ?? model.pending.last) { outboxId in
                        if let text = model.undoSend(outboxId) { draft = text }
                    }
                    .padding(.bottom, 12)
                }
                Divider()
                ComposerView(replyLabel: conversation.replyLabel, text: $draft) {
                    if model.send(draft) { draft = "" }
                }
            }
            .id(conversation.id)
        } else {
            VStack(spacing: 8) {
                Image(systemName: "envelope").font(.system(size: 36)).foregroundStyle(.tertiary)
                Text("Select a conversation").foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func header(_ conversation: Conversation) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title).font(.headline).lineLimit(1)
                Text(conversation.subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            Button {
                model.togglePin()
            } label: {
                Image(systemName: conversation.pinned ? "star.fill" : "star")
            }
            .help(conversation.pinned ? "Unpin" : "Pin")
            .accessibilityLabel(conversation.pinned ? "Unpin" : "Pin")
            Button {
                model.markDoneAndAdvance()
            } label: {
                Image(systemName: "checkmark")
            }
            .help("Done (⌘E)")
            .accessibilityLabel("Done")
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

/// "Sending in 8s · Undo" while a reply waits in its undo window.
struct UndoBar: View {
    let pending: PendingSend?
    let undo: (Int64) -> Void

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.5)) { context in
            let left = pending.map { Int(ceil(Double($0.sendAt) - context.date.timeIntervalSince1970)) } ?? 0
            if let pending, left > 0 {
                HStack(spacing: 14) {
                    Text("Sending in \(left)s").foregroundStyle(.white)
                    Button("Undo") { undo(pending.outboxId) }
                        .buttonStyle(.plain)
                        .foregroundStyle(Color(hex: "#8fb6ff"))
                        .fontWeight(.semibold)
                }
                .font(.callout)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(Capsule().fill(Color.black.opacity(0.85)))
            }
        }
    }
}

/// The reply box. Return sends; Option-Return starts a new line.
struct ComposerView: View {
    let replyLabel: String
    @Binding var text: String
    let send: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(replyLabel).font(.caption).foregroundStyle(.secondary)
            HStack(alignment: .bottom, spacing: 10) {
                TextField("Message", text: $text, axis: .vertical)
                    .textFieldStyle(.plain)
                    .lineLimit(1...8)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(RoundedRectangle(cornerRadius: 18).fill(Theme.bubbleOther))
                    .overlay(
                        RoundedRectangle(cornerRadius: 18)
                            .stroke(focused ? Theme.accent : .clear, lineWidth: 2)
                    )
                    .focused($focused)
                    .onSubmit(submit)
                    .accessibilityLabel("Reply")
                Button(action: submit) {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 28))
                }
                .buttonStyle(.borderless)
                .foregroundStyle(canSend ? Theme.accent : Color.secondary)
                .disabled(!canSend)
                .accessibilityLabel("Send")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .onReceive(NotificationCenter.default.publisher(for: .focusComposer)) { _ in focused = true }
    }

    private var canSend: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func submit() {
        if canSend { send() }
    }
}
