import AppKit
import SwiftUI

/// First run: no accounts yet. Add one, or look around the demo first.
/// Also explains when MailHerd couldn't open its saved mail.
struct WelcomeView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Welcome to MailHerd").font(.largeTitle.bold())
                if model.startupError.isEmpty {
                    Text("""
                        Add your email accounts to get started. Your mail is kept on this Mac, \
                        encrypted, and your passwords stay in your keychain.
                        """)
                        .foregroundStyle(.secondary)
                    AddAccountView(onFinished: {})
                } else {
                    startupProblem
                }
                Divider()
                Button("Or explore the demo with sample conversations") { model.useDemo() }
                    .buttonStyle(.link)
                    .frame(maxWidth: .infinity)
            }
            .padding(28)
            .frame(width: 460)
            .background(RoundedRectangle(cornerRadius: 16).fill(Theme.card))
            .overlay(RoundedRectangle(cornerRadius: 16).stroke(Color.secondary.opacity(0.25)))
            .padding(40)
            .frame(maxWidth: .infinity)
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var advice: String {
        if model.canStartFresh {
            return "Your mail is safe on the servers. Start fresh deletes MailHerd's copy on this Mac; "
                + "then add your accounts again and your mail downloads again. "
                + "Replies still waiting to be sent are lost."
        }
        return "MailHerd keeps its passwords in your keychain. Make sure you're signed in "
            + "to this Mac as usual, then try again."
    }

    private var startupProblem: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("MailHerd couldn't start.").foregroundStyle(.secondary)
            Text(model.startupError + "\n\n" + advice)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(Theme.danger.opacity(0.1)))
                .textSelection(.enabled)
            HStack {
                Button("Try again") { model.openAccounts() }
                Spacer()
                if model.canStartFresh {
                    Button("Start fresh") { model.startFresh() }
                }
            }
        }
    }
}

/// Adding an account: pick a provider, then follow the sign-in. The steps
/// come from the core as `model.signIn`.
struct AddAccountView: View {
    @EnvironmentObject var model: AppModel
    let onFinished: () -> Void
    @State private var icloudForm = false
    @State private var email = ""
    @State private var password = ""

    var body: some View {
        Group {
            switch model.signIn {
            case nil:
                if icloudForm { icloud } else { providers }
            case .browser(let url)?:
                waiting("Finish signing in in your browser, then come back here.") {
                    HStack {
                        Button("Cancel") { model.dismissSignIn() }
                        Spacer()
                        Button("Open the sign-in page again") {
                            if let link = URL(string: url) { NSWorkspace.shared.open(link) }
                        }
                        .buttonStyle(.link)
                    }
                }
            case .checking?:
                waiting("Checking with iCloud…") { EmptyView() }
            case .done(let email)?:
                done(email)
            case .failed(let message)?:
                failed(message)
            }
        }
        .onAppear {
            if let prefill = model.icloudEmail {
                email = prefill
                icloudForm = true
            }
        }
    }

    private var providers: some View {
        VStack(spacing: 8) {
            provider("Gmail", "Sign in with Google in your browser", "#ea4335") {
                model.startSignIn("google")
            }
            provider("Microsoft 365", "Work or school account, in your browser", "#0078d4") {
                model.startSignIn("microsoft")
            }
            provider("iCloud Mail", "Uses an app-specific password", "#34aadc") {
                icloudForm = true
            }
        }
    }

    private func provider(_ name: String, _ detail: String, _ color: String,
                          action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Circle().fill(Color(hex: color)).frame(width: 12, height: 12)
                VStack(alignment: .leading, spacing: 1) {
                    Text(name).fontWeight(.semibold)
                    Text(detail).font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(.tertiary)
            }
            .padding(12)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.3)))
    }

    private var icloud: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Add iCloud Mail").font(.title3.bold())
            Text("""
                Apple doesn't let mail apps use your Apple Account password. Instead, make an \
                app-specific password: at account.apple.com, go to Sign-In and Security, then \
                App-Specific Passwords, and create one called MailHerd.
                """)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Button("Open account.apple.com") {
                if let link = URL(string: "https://account.apple.com/account/manage") {
                    NSWorkspace.shared.open(link)
                }
            }
            .buttonStyle(.link)
            TextField("you@icloud.com", text: $email)
                .textContentType(.username)
            SecureField("App-specific password (xxxx-xxxx-xxxx-xxxx)", text: $password)
                .onSubmit(addICloud)
            HStack {
                Button("Back") { icloudForm = false }
                Spacer()
                Button("Add account", action: addICloud)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canAddICloud)
            }
        }
        .textFieldStyle(.roundedBorder)
    }

    private var canAddICloud: Bool {
        email.contains("@") && !password.isEmpty
    }

    private func addICloud() {
        if canAddICloud { model.startSignIn("icloud", email: email, password: password) }
    }

    private func waiting<Actions: View>(_ message: String,
                                        @ViewBuilder actions: () -> Actions) -> some View {
        VStack(spacing: 12) {
            ProgressView()
            Text(message).foregroundStyle(.secondary).multilineTextAlignment(.center)
            actions()
        }
        .frame(maxWidth: .infinity)
    }

    private func done(_ address: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "checkmark.circle.fill").font(.system(size: 34)).foregroundStyle(Theme.accent)
            Text("Added \(address). Your mail is on its way — recent messages come first.")
                .multilineTextAlignment(.center)
            HStack {
                Button("Add another account") {
                    reset()
                    model.dismissSignIn()
                }
                .buttonStyle(.link)
                Spacer()
                Button("Done") {
                    reset()
                    model.dismissSignIn()
                    onFinished()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .frame(maxWidth: .infinity)
    }

    private func failed(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(message)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(Theme.danger.opacity(0.1)))
                .textSelection(.enabled)
            HStack {
                Button("Cancel") {
                    reset()
                    model.dismissSignIn()
                    onFinished()
                }
                Spacer()
                // Back to the form (iCloud keeps what was typed) or the provider list.
                Button("Try again") { model.dismissSignIn() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    private func reset() {
        icloudForm = false
        password = ""
    }
}
