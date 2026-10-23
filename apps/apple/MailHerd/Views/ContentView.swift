import AppKit
import SwiftUI

struct ContentView: View {
    @EnvironmentObject var model: AppModel
    @State private var removing: AccountRow?

    var body: some View {
        Group {
            if model.showWelcome {
                WelcomeView()
            } else {
                NavigationSplitView {
                    SidebarView(removing: $removing)
                        .navigationSplitViewColumnWidth(min: 200, ideal: 236, max: 320)
                } content: {
                    MessageListView()
                        .navigationSplitViewColumnWidth(min: 300, ideal: 380, max: 520)
                } detail: {
                    ConversationView()
                }
            }
        }
        .navigationTitle(model.conversation?.title ?? "MailHerd")
        .sheet(isPresented: $model.addingAccount, onDismiss: {
            model.icloudEmail = nil
            model.dismissSignIn()
        }) {
            AddAccountSheet()
                .environmentObject(model)
        }
        .confirmationDialog(
            "Remove \(removing?.email ?? "")?",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                if let id = removing?.id { model.removeAccount(id) }
                removing = nil
            }
            Button("Cancel", role: .cancel) { removing = nil }
        } message: {
            Text("MailHerd will stop checking this account and delete its mail from this Mac. "
                + "Nothing changes in the account itself — your mail stays online.")
        }
    }
}

/// The Add account sheet over the main window.
struct AddAccountSheet: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("Add an account").font(.title2.bold())
                Spacer()
                Button {
                    model.addingAccount = false
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .keyboardShortcut(.cancelAction)
                .accessibilityLabel("Close")
            }
            AddAccountView(onFinished: { model.addingAccount = false })
        }
        .padding(24)
        .frame(width: 460)
    }
}
