import AppKit
import Foundation

/// The window's state. Everything comes from the Rust core: this class only
/// decodes the view models, re-reads them when the core's revision changes,
/// and passes actions back. No mail logic lives here.
@MainActor
final class AppModel: ObservableObject {
    let core: MailHerdApp

    @Published private(set) var sidebar: Sidebar?
    @Published private(set) var rows: [ListRow] = []
    @Published private(set) var conversation: Conversation?
    @Published private(set) var pending: [PendingSend] = []
    @Published private(set) var signIn: SignInState?
    @Published private(set) var showWelcome = false
    @Published private(set) var startupError = ""
    @Published private(set) var canStartFresh = false
    @Published private(set) var isDemo = false
    @Published private(set) var mode = "threads"
    @Published private(set) var mailbox = Mailbox.inbox
    @Published private(set) var lastError = ""

    /// The open conversation: a thread id, or "people:<key>".
    @Published var selectedId: String? {
        didSet {
            guard selectedId != oldValue else { return }
            if let id = selectedId { core.openConversation(id: id) }
            refresh(force: true)
        }
    }

    /// The Add account sheet, over the main window.
    @Published var addingAccount = false
    /// The iCloud form, prefilled when signing in again.
    @Published var icloudEmail: String?

    private var seenRevision: UInt64?
    private var timer: Timer?
    private var openedSignInURL: String?

    init() {
        core = MailHerdApp.launch(utcOffsetSeconds: Int32(TimeZone.current.secondsFromGMT()))
        refresh(force: true)
        // Brings in synced mail and sends replies once their undo time is up.
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
        // For screenshots: open the first conversation straight away.
        // (After init, so the selection's didSet runs.)
        if ProcessInfo.processInfo.environment["MAILHERD_OPEN_FIRST"] == "1" {
            DispatchQueue.main.async { self.selectedId = self.rows.first?.id }
        }
    }

    private func tick() {
        _ = core.tick()
        refresh()
    }

    /// Re-read the views if anything changed since the last read.
    func refresh(force: Bool = false) {
        let revision = core.revision()
        guard force || revision != seenRevision else { return }
        seenRevision = revision

        showWelcome = core.showWelcome()
        startupError = core.startupError()
        canStartFresh = core.canStartFresh()
        isDemo = core.isDemo()
        mode = core.mode()
        mailbox = CoreJSON.decode(Mailbox.self, core.mailboxJson()) ?? .inbox
        lastError = core.lastError()
        sidebar = CoreJSON.decode(Sidebar.self, core.sidebarJson())
        if mode == "people" {
            rows = (CoreJSON.decode([PersonRow].self, core.listJson()) ?? []).map(ListRow.init)
        } else {
            rows = (CoreJSON.decode([ThreadRow].self, core.listJson()) ?? []).map(ListRow.init)
        }
        conversation = selectedId.flatMap { CoreJSON.decode(Conversation.self, core.conversationJson(id: $0)) }
        pending = CoreJSON.decode([PendingSend].self, core.pendingJson()) ?? []

        let state = CoreJSON.decode(SignInState.self, core.signInJson())
        if state != signIn {
            signIn = state
            // Open the provider's sign-in page in the browser, once per attempt.
            if case .browser(let url) = state, url != openedSignInURL, let link = URL(string: url) {
                openedSignInURL = url
                NSWorkspace.shared.open(link)
            }
            // The first account added from the welcome screen carries on in the
            // Add account sheet, so the user sees it worked and can add the others.
            if case .done = state, showWelcome == false { addingAccount = true }
        }
    }

    // MARK: Lists

    func select(_ mailbox: Mailbox) {
        core.selectMailbox(mailboxJson: mailbox.json)
        selectedId = nil
        refresh()
    }

    func setMode(_ mode: String) {
        core.setListMode(mode: mode)
        selectedId = nil
        refresh()
    }

    /// Move the selection up or down the list.
    func move(_ step: Int) {
        guard !rows.isEmpty else { return }
        let index = rows.firstIndex { $0.id == selectedId } ?? (step > 0 ? -1 : rows.count)
        selectedId = rows[max(0, min(rows.count - 1, index + step))].id
    }

    // MARK: Conversation

    func togglePin() {
        guard let id = selectedId else { return }
        core.togglePin(id: id)
        refresh()
    }

    /// Done, then move to the next conversation, like most mail apps.
    func markDoneAndAdvance() {
        guard let id = selectedId, let index = rows.firstIndex(where: { $0.id == id }) else { return }
        let next = rows.count > 1 ? rows[index + 1 < rows.count ? index + 1 : index - 1].id : nil
        core.markDone(id: id)
        selectedId = next
        refresh()
    }

    func markUnread() {
        guard let id = selectedId, !id.hasPrefix("people:") else { return }
        core.markUnread(id: id)
        selectedId = nil
        refresh()
    }

    /// Queue a reply. Returns false if it couldn't be queued.
    func send(_ text: String) -> Bool {
        guard let id = selectedId else { return false }
        let queued = !core.sendReply(id: id, text: text).isEmpty
        refresh()
        return queued
    }

    /// Cancel a send. Returns its text for the reply box, if it hadn't gone yet.
    func undoSend(_ outboxId: Int64) -> String? {
        let text = core.undoSend(outboxId: outboxId)
        refresh()
        return text.isEmpty ? nil : text
    }

    func html(_ messageId: Int64) -> String {
        core.messageHtml(id: messageId)
    }

    // MARK: Accounts

    /// `provider` is "google", "microsoft" or "icloud".
    func startSignIn(_ provider: String, email: String = "", password: String = "") {
        core.startSignIn(provider: provider, email: email, password: password)
        refresh()
    }

    func dismissSignIn() {
        core.dismissSignIn()
        refresh()
    }

    func signInAgain(_ account: AccountRow) {
        if account.provider == "iCloud" {
            icloudEmail = account.email
        } else {
            startSignIn(account.provider == "Gmail" ? "google" : "microsoft")
        }
        addingAccount = true
    }

    func removeAccount(_ id: String) {
        core.removeAccount(id: id)
        selectedId = nil
        refresh()
    }

    func syncNow() {
        core.syncNow()
    }

    func useDemo() {
        core.useDemo()
        selectedId = nil
        refresh()
    }

    func openAccounts() {
        core.openAccounts()
        selectedId = nil
        refresh()
    }

    func startFresh() {
        core.startFresh()
        selectedId = nil
        refresh()
    }
}
