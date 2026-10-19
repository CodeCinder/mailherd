import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import LanternPost

ApplicationWindow {
    id: window

    width: 1240
    height: 800
    minimumWidth: 900
    minimumHeight: 540
    visible: true
    title: conversation ? qsTr("%1 — LanternPost").arg(conversation.title) : qsTr("LanternPost")
    color: theme.window

    Theme { id: theme }
    AppController { id: app }

    property string selectedId: ""

    // View models from the core, re-read whenever app.revision changes.
    readonly property var sidebarModel: { app.revision; return JSON.parse(app.sidebarJson()) }
    readonly property var listModel: { app.revision; return JSON.parse(app.listJson()) }
    readonly property var conversation: {
        app.revision
        return selectedId !== "" ? JSON.parse(app.conversationJson(selectedId)) : null
    }
    readonly property var pending: { app.revision; return JSON.parse(app.pendingJson()) }

    readonly property string mailboxTitle: {
        if (!sidebarModel)
            return ""
        for (var i = 0; i < sidebarModel.items.length; i++)
            if (JSON.stringify(sidebarModel.items[i].mailbox) === app.mailbox)
                return sidebarModel.items[i].label
        return qsTr("Inbox")
    }

    Component.onCompleted: app.setUtcOffset(-new Date().getTimezoneOffset() * 60)

    function select(id) {
        selectedId = id
        if (id !== "")
            app.openConversation(id)
    }

    // After Done, move to the next conversation, like most mail apps.
    function doneAndAdvance() {
        if (selectedId === "")
            return
        var i = messageList.indexOfSelected()
        var rows = messageList.rows
        var next = rows.length > 1 ? rows[i + 1 < rows.length ? i + 1 : i - 1].id : ""
        app.markDone(selectedId)
        select(next)
    }

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: app.tick()
    }

    // Single-key shortcuts, off while typing.
    readonly property bool typing: conversationPane.composer.editing
    Shortcut { sequence: "j"; enabled: !window.typing; onActivated: messageList.move(1) }
    Shortcut { sequence: "k"; enabled: !window.typing; onActivated: messageList.move(-1) }
    Shortcut { sequence: "e"; enabled: !window.typing; onActivated: window.doneAndAdvance() }
    Shortcut { sequence: "p"; enabled: !window.typing && window.selectedId !== ""; onActivated: app.togglePin(window.selectedId) }
    Shortcut { sequence: "u"; enabled: !window.typing && window.selectedId !== "" && window.selectedId.indexOf("people:") !== 0; onActivated: { app.markUnread(window.selectedId); window.selectedId = "" } }
    Shortcut { sequence: "r"; enabled: !window.typing && window.selectedId !== ""; onActivated: conversationPane.composer.focusInput() }
    Shortcut { sequence: "t"; enabled: !window.typing; onActivated: { app.setListMode(app.mode === "people" ? "threads" : "people"); window.selectedId = "" } }
    Shortcut { sequence: "?"; enabled: !window.typing; onActivated: help.open() }
    Shortcut { sequence: "Esc"; onActivated: conversationPane.composer.clearFocus() }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Sidebar {
            theme: theme
            model: window.sidebarModel
            currentMailbox: app.mailbox
            demo: app.demo
            Layout.preferredWidth: 236
            Layout.fillHeight: true
            onMailboxSelected: function (json) {
                app.selectMailbox(json)
                window.selectedId = ""
            }
        }

        Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: theme.border }

        MessageList {
            id: messageList
            theme: theme
            title: window.mailboxTitle
            mode: app.mode
            model: window.listModel
            selectedId: window.selectedId
            Layout.preferredWidth: 380
            Layout.fillHeight: true
            onSelected: function (id) { window.select(id) }
            onModeRequested: function (mode) {
                app.setListMode(mode)
                window.selectedId = ""
            }
        }

        Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: theme.border }

        Conversation {
            id: conversationPane
            theme: theme
            conversation: window.conversation
            pending: window.pending
            htmlLookup: function (id) { return app.messageHtml(id) }
            Layout.fillWidth: true
            Layout.fillHeight: true
            onPinRequested: app.togglePin(window.selectedId)
            onDoneRequested: window.doneAndAdvance()
            onSend: function (text) {
                var result = app.sendReply(window.selectedId, text)
                if (result !== "")
                    conversationPane.composer.text = ""
            }
            onUndo: function (id) {
                var text = app.undoSend(id)
                if (text !== "") {
                    conversationPane.composer.text = text
                    conversationPane.composer.focusInput()
                }
            }
        }
    }

    Dialog {
        id: help
        title: qsTr("Keyboard shortcuts")
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Close
        GridLayout {
            columns: 2
            columnSpacing: 24
            rowSpacing: 6
            Repeater {
                model: [
                    "J / K", qsTr("Next / previous conversation"),
                    "E", qsTr("Done (archive)"),
                    "P", qsTr("Pin or unpin"),
                    "U", qsTr("Mark unread"),
                    "R", qsTr("Reply"),
                    "T", qsTr("Switch Threads / People"),
                    "Enter", qsTr("Send (Shift+Enter for a new line)"),
                    "Esc", qsTr("Leave the reply box"),
                    "?", qsTr("This help")
                ]
                delegate: Text {
                    required property var modelData
                    required property int index
                    text: modelData
                    font.bold: index % 2 === 0
                }
            }
        }
    }
}
