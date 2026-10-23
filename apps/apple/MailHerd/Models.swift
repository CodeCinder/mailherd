import Foundation

// The view models the Rust core sends as JSON (see core/crates/mh-app/src/view.rs).
// Keys arrive in snake_case and are decoded with .convertFromSnakeCase.

struct Avatar: Decodable, Hashable {
    let initials: String
    let color: String
}

/// Which list is showing. Sent back to the core as JSON when selected.
struct Mailbox: Codable, Hashable {
    let kind: String
    let account: String?

    static let inbox = Mailbox(kind: "inbox", account: nil)

    var json: String {
        guard let data = try? JSONEncoder().encode(self) else { return #"{"kind":"inbox"}"# }
        return String(decoding: data, as: UTF8.self)
    }
}

struct SidebarItem: Decodable, Identifiable {
    let mailbox: Mailbox
    let label: String
    let icon: String
    let unreadCount: Int
    let color: String?

    var id: Mailbox { mailbox }
    var isAccount: Bool { icon == "account" }
}

struct AccountRow: Decodable, Identifiable {
    let id: String
    let email: String
    let displayName: String
    let provider: String
    let color: String
    let unreadCount: Int
    let status: String
    let needsSignIn: Bool
}

struct Sidebar: Decodable {
    let items: [SidebarItem]
    let accounts: [AccountRow]

    func account(_ id: String?) -> AccountRow? {
        accounts.first { $0.id == id }
    }
}

struct ThreadRow: Decodable, Identifiable {
    let threadId: String
    let subject: String
    let participants: String
    let snippet: String
    let dateLabel: String
    let unread: Bool
    let messageCount: Int
    let pinned: Bool
    let hasAttachments: Bool
    let accountColors: [String]
    let avatar: Avatar

    var id: String { threadId }
}

struct PersonRow: Decodable, Identifiable {
    let key: String
    let name: String
    let isGroup: Bool
    let snippet: String
    let dateLabel: String
    let unread: Bool
    let threadCount: Int
    let pinned: Bool
    let avatar: Avatar

    /// What the core expects when opening a People conversation.
    var id: String { "people:" + key }
}

/// One row of either list, so the list and the keyboard work the same for both.
struct ListRow: Identifiable {
    let id: String
    let title: String
    let detail: String?
    let snippet: String
    let dateLabel: String
    let unread: Bool
    let pinned: Bool
    let count: Int
    let accountColors: [String]
    let avatar: Avatar

    init(_ t: ThreadRow) {
        id = t.threadId
        title = t.participants
        detail = t.subject
        snippet = t.snippet
        dateLabel = t.dateLabel
        unread = t.unread
        pinned = t.pinned
        count = t.messageCount
        accountColors = t.accountColors
        avatar = t.avatar
    }

    init(_ p: PersonRow) {
        id = p.id
        title = p.name
        detail = p.threadCount > 1 ? "\(p.threadCount) conversations" : nil
        snippet = p.snippet
        dateLabel = p.dateLabel
        unread = p.unread
        pinned = p.pinned
        count = 0
        accountColors = []
        avatar = p.avatar
    }
}

struct AttachmentRow: Decodable, Hashable {
    let filename: String
    let mimeType: String
    let sizeLabel: String
    let isImage: Bool
}

struct Bubble: Decodable, Identifiable {
    let messageId: Int64
    let senderName: String
    let senderEmail: String
    let isMe: Bool
    let text: String
    let quoted: String?
    let signature: String?
    let forwarded: Bool
    let hasHidden: Bool
    let timeLabel: String
    let avatar: Avatar
    let attachments: [AttachmentRow]
    let isCard: Bool
    let hasHtml: Bool
    let accountColors: [String]
    let delivery: String
    let subjectChange: String?
    let showSender: Bool

    var id: Int64 { messageId }
    var sending: Bool { delivery == "sending" }
    var failed: Bool { delivery == "failed" }
}

enum ConversationItem: Decodable, Identifiable {
    case day(label: String)
    case message(Bubble)

    private enum Keys: String, CodingKey { case type, label }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        if try c.decode(String.self, forKey: .type) == "day" {
            self = .day(label: try c.decode(String.self, forKey: .label))
        } else {
            self = .message(try Bubble(from: decoder))
        }
    }

    var id: String {
        switch self {
        case .day(let label): return "day:" + label
        case .message(let b): return "msg:\(b.messageId)"
        }
    }
}

struct Conversation: Decodable {
    let id: String
    let title: String
    let subtitle: String
    let isGroup: Bool
    let pinned: Bool
    let items: [ConversationItem]
    let replyLabel: String
}

struct PendingSend: Decodable, Identifiable {
    let outboxId: Int64
    let threadId: String
    let sendAt: Int64

    var id: Int64 { outboxId }
}

/// Progress of adding an account.
enum SignInState: Decodable, Equatable {
    case browser(url: String)
    case checking
    case done(email: String)
    case failed(message: String)

    private enum Keys: String, CodingKey { case state, url, email, message }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        switch try c.decode(String.self, forKey: .state) {
        case "browser": self = .browser(url: try c.decode(String.self, forKey: .url))
        case "done": self = .done(email: try c.decode(String.self, forKey: .email))
        case "failed": self = .failed(message: try c.decode(String.self, forKey: .message))
        default: self = .checking
        }
    }
}

enum CoreJSON {
    private static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()

    /// Decode what the core sent; nil if it is "null" or unexpected.
    static func decode<T: Decodable>(_ type: T.Type, _ json: String) -> T? {
        try? decoder.decode(T.self, from: Data(json.utf8))
    }
}
