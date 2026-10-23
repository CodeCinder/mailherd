import AppKit
import SwiftUI

/// Left column: mailboxes, then one entry per account.
struct SidebarView: View {
    @EnvironmentObject var model: AppModel
    @Binding var removing: AccountRow?

    private func symbol(_ icon: String) -> String {
        switch icon {
        case "inbox": return "tray"
        case "pin": return "star"
        case "send": return "paperplane"
        case "check": return "checkmark.circle"
        default: return "circle"
        }
    }

    private var selection: Binding<Mailbox?> {
        Binding(get: { model.mailbox }, set: { if let m = $0 { model.select(m) } })
    }

    var body: some View {
        let items = model.sidebar?.items ?? []
        List(selection: selection) {
            Section {
                ForEach(items.filter { !$0.isAccount }) { item in
                    Label(item.label, systemImage: symbol(item.icon))
                        .badge(item.unreadCount)
                        .tag(item.mailbox)
                }
            }
            let accounts = items.filter(\.isAccount)
            if !accounts.isEmpty {
                Section("Accounts") {
                    ForEach(accounts) { item in
                        accountRow(item, model.sidebar?.account(item.mailbox.account))
                            .tag(item.mailbox)
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) { footer }
    }

    private func accountRow(_ item: SidebarItem, _ account: AccountRow?) -> some View {
        HStack(spacing: 8) {
            Circle()
                .fill(Color(hex: item.color ?? "#8a9099"))
                .frame(width: 10, height: 10)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.label).lineLimit(1)
                if let status = account?.status, !status.isEmpty {
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(account?.needsSignIn == true ? Theme.danger : .secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 4)
            if let account, account.needsSignIn {
                Button("Sign in") { model.signInAgain(account) }
                    .controlSize(.small)
            }
        }
        .badge(item.unreadCount)
        .contextMenu {
            if let account, !model.isDemo {
                Button("Check for New Mail") { model.syncNow() }
                Button("Sign In Again") { model.signInAgain(account) }
                Divider()
                Button("Remove from MailHerd…") { removing = account }
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            if model.isDemo {
                Text("Demo mode: sample conversations, nothing is sent.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Theme.accent.opacity(0.12)))
                Button("Add your accounts") { model.openAccounts() }
                    .buttonStyle(.link)
            } else {
                Button {
                    model.addingAccount = true
                } label: {
                    Label("Add account", systemImage: "plus")
                }
                .buttonStyle(.link)
            }
        }
        .padding(12)
    }
}
