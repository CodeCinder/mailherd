import SwiftUI

@main
struct MailHerdMain: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .frame(minWidth: 900, minHeight: 540)
        }
        .defaultSize(width: 1240, height: 800)
        .commands {
            CommandGroup(after: .newItem) {
                Button("Add Account…") { model.addingAccount = true }
                    .disabled(model.isDemo || model.showWelcome)
            }
            CommandMenu("Mailbox") {
                Button("Check for New Mail") { model.syncNow() }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                Divider()
                Button("Threads") { model.setMode("threads") }
                    .keyboardShortcut("1", modifiers: .command)
                Button("People") { model.setMode("people") }
                    .keyboardShortcut("2", modifiers: .command)
            }
            CommandMenu("Conversation") {
                Button("Next") { model.move(1) }
                    .keyboardShortcut(.downArrow, modifiers: .command)
                Button("Previous") { model.move(-1) }
                    .keyboardShortcut(.upArrow, modifiers: .command)
                Divider()
                Button("Done") { model.markDoneAndAdvance() }
                    .keyboardShortcut("e", modifiers: .command)
                    .disabled(model.selectedId == nil)
                Button("Pin or Unpin") { model.togglePin() }
                    .keyboardShortcut("p", modifiers: [.command, .shift])
                    .disabled(model.selectedId == nil)
                Button("Mark as Unread") { model.markUnread() }
                    .keyboardShortcut("u", modifiers: [.command, .shift])
                    .disabled(model.selectedId == nil || model.selectedId?.hasPrefix("people:") == true)
                Button("Reply") { NotificationCenter.default.post(name: .focusComposer, object: nil) }
                    .keyboardShortcut("r", modifiers: .command)
                    .disabled(model.selectedId == nil)
            }
        }
    }
}

extension Notification.Name {
    /// Put the cursor in the reply box.
    static let focusComposer = Notification.Name("MailHerdFocusComposer")
}
