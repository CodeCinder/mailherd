import AppKit
import SwiftUI

/// Middle column: conversations as Threads or grouped by People.
struct MessageListView: View {
    @EnvironmentObject var model: AppModel

    private var title: String {
        model.sidebar?.items.first { $0.mailbox == model.mailbox }?.label ?? "Inbox"
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(title).font(.title2.bold())
                Spacer()
                Picker("View", selection: Binding(get: { model.mode }, set: { model.setMode($0) })) {
                    Text("Threads").tag("threads")
                    Text("People").tag("people")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 170)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)

            Divider()

            if model.rows.isEmpty {
                Spacer()
                Text("Nothing here").foregroundStyle(.secondary)
                Spacer()
            } else {
                List(model.rows, selection: $model.selectedId) { row in
                    RowView(row: row).tag(row.id)
                }
                .listStyle(.inset)
            }
        }
    }
}

private struct RowView: View {
    let row: ListRow

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Circle()
                .fill(row.unread ? Theme.accent : .clear)
                .frame(width: 8, height: 8)
                .padding(.top, 16)
            AvatarView(avatar: row.avatar)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline) {
                    Text(row.title)
                        .fontWeight(row.unread ? .bold : .regular)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    if row.count > 1 {
                        Text("\(row.count)").font(.caption).foregroundStyle(.secondary)
                    }
                    if row.pinned {
                        Image(systemName: "star.fill").font(.caption).foregroundStyle(.yellow)
                    }
                    Text(row.dateLabel)
                        .font(.caption)
                        .foregroundStyle(row.unread ? Theme.accent : .secondary)
                        .fontWeight(row.unread ? .semibold : .regular)
                }
                if let detail = row.detail {
                    Text(detail)
                        .font(.callout)
                        .fontWeight(row.unread ? .semibold : .regular)
                        .lineLimit(1)
                }
                HStack(alignment: .top) {
                    Text(row.snippet)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                    Spacer(minLength: 4)
                    AccountDots(colors: row.accountColors)
                        .padding(.top, 4)
                }
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(row.unread ? "Unread. \(row.title). \(row.detail ?? ""). \(row.snippet)"
                                       : "\(row.title). \(row.detail ?? ""). \(row.snippet)")
    }
}
