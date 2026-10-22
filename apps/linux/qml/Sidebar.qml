import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Left pane: mailboxes, then one entry per account.
Rectangle {
    id: sidebar

    property Theme theme
    property var model: null
    property string currentMailbox: ""
    property bool demo: false
    signal mailboxSelected(string mailboxJson)
    signal addAccountRequested()
    signal leaveDemoRequested()
    signal syncRequested()
    signal signInAgainRequested(var account)
    signal removeRequested(var account)

    color: theme.sidebar

    function accountFor(id) {
        var rows = model ? model.accounts : []
        for (var i = 0; i < rows.length; i++)
            if (rows[i].id === id)
                return rows[i]
        return null
    }

    function iconFor(name) {
        switch (name) {
        case "inbox": return "✉"
        case "pin": return "★"
        case "send": return "➤"
        case "check": return "✓"
        default: return "●"
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 2

        Text {
            text: qsTr("MailHerd")
            color: sidebar.theme.text
            font.pointSize: sidebar.theme.title
            font.bold: true
            Layout.bottomMargin: 14
            Layout.leftMargin: 8
            Layout.topMargin: 4
        }

        Repeater {
            model: sidebar.model ? sidebar.model.items : []

            delegate: ColumnLayout {
                id: entry
                required property var modelData
                required property int index
                readonly property bool isAccount: modelData.icon === "account"
                readonly property bool firstAccount: isAccount && index > 0
                    && sidebar.model.items[index - 1].icon !== "account"
                readonly property bool selected: JSON.stringify(modelData.mailbox) === sidebar.currentMailbox
                readonly property var account: isAccount ? sidebar.accountFor(modelData.mailbox.account) : null
                readonly property string status: account ? account.status : ""
                Layout.fillWidth: true
                spacing: 0

                Text {
                    visible: entry.firstAccount
                    text: qsTr("Accounts")
                    color: sidebar.theme.faint
                    font.pointSize: sidebar.theme.tiny
                    font.bold: true
                    font.capitalization: Font.AllUppercase
                    Layout.topMargin: 18
                    Layout.bottomMargin: 6
                    Layout.leftMargin: 8
                }

                ItemDelegate {
                    id: button
                    Layout.fillWidth: true
                    implicitHeight: entry.status !== "" ? 46 : 34
                    focusPolicy: Qt.StrongFocus
                    Accessible.name: entry.modelData.unread_count > 0
                        ? qsTr("%1, %2 unread").arg(entry.modelData.label).arg(entry.modelData.unread_count)
                        : entry.modelData.label
                    onClicked: sidebar.mailboxSelected(JSON.stringify(entry.modelData.mailbox))

                    TapHandler {
                        enabled: entry.account !== null && !sidebar.demo
                        acceptedButtons: Qt.RightButton
                        onTapped: accountMenu.popup()
                    }

                    Menu {
                        id: accountMenu
                        MenuItem { text: qsTr("Check for new mail"); onTriggered: sidebar.syncRequested() }
                        MenuItem {
                            text: qsTr("Sign in again")
                            onTriggered: sidebar.signInAgainRequested(entry.account)
                        }
                        MenuSeparator {}
                        MenuItem {
                            text: qsTr("Remove from MailHerd…")
                            onTriggered: sidebar.removeRequested(entry.account)
                        }
                    }

                    background: Rectangle {
                        radius: 8
                        color: entry.selected ? sidebar.theme.selection
                            : (button.hovered ? sidebar.theme.hover : "transparent")
                        border.width: button.visualFocus ? 2 : 0
                        border.color: sidebar.theme.accent
                    }

                    contentItem: RowLayout {
                        spacing: 10
                        Item {
                            implicitWidth: 18
                            implicitHeight: 18
                            Rectangle {
                                visible: entry.isAccount
                                anchors.centerIn: parent
                                width: 10; height: 10; radius: 5
                                color: entry.modelData.color || sidebar.theme.faint
                            }
                            Text {
                                visible: !entry.isAccount
                                anchors.centerIn: parent
                                text: sidebar.iconFor(entry.modelData.icon)
                                color: entry.selected ? sidebar.theme.accent : sidebar.theme.subtext
                                font.pointSize: sidebar.theme.base
                            }
                        }
                        ColumnLayout {
                            spacing: 1
                            Layout.fillWidth: true
                            Text {
                                text: entry.modelData.label
                                color: sidebar.theme.text
                                font.pointSize: entry.isAccount ? sidebar.theme.small : sidebar.theme.base
                                font.bold: entry.selected
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                visible: entry.status !== ""
                                text: entry.status
                                color: entry.account && entry.account.needs_sign_in
                                    ? sidebar.theme.danger : sidebar.theme.faint
                                font.pointSize: sidebar.theme.tiny
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }
                        Button {
                            visible: entry.account !== null && entry.account.needs_sign_in
                            text: qsTr("Sign in")
                            font.pointSize: sidebar.theme.tiny
                            implicitHeight: 24
                            onClicked: sidebar.signInAgainRequested(entry.account)
                        }
                        Rectangle {
                            visible: entry.modelData.unread_count > 0
                            radius: height / 2
                            color: entry.selected ? sidebar.theme.accent : sidebar.theme.border
                            implicitHeight: 18
                            implicitWidth: Math.max(18, count.implicitWidth + 10)
                            Text {
                                id: count
                                anchors.centerIn: parent
                                text: entry.modelData.unread_count
                                color: entry.selected ? sidebar.theme.accentText : sidebar.theme.text
                                font.pointSize: sidebar.theme.tiny
                                font.bold: true
                            }
                        }
                    }
                }
            }
        }

        Item { Layout.fillHeight: true }

        Rectangle {
            visible: sidebar.demo
            Layout.fillWidth: true
            radius: 8
            color: sidebar.theme.selection
            implicitHeight: demoText.implicitHeight + 16
            Text {
                id: demoText
                anchors.fill: parent
                anchors.margins: 8
                wrapMode: Text.WordWrap
                text: qsTr("Demo mode: sample conversations, nothing is sent.")
                color: sidebar.theme.subtext
                font.pointSize: sidebar.theme.tiny
            }
        }

        ItemDelegate {
            id: addButton
            Layout.fillWidth: true
            implicitHeight: 34
            text: sidebar.demo ? qsTr("Add your accounts") : qsTr("+  Add account")
            onClicked: sidebar.demo ? sidebar.leaveDemoRequested() : sidebar.addAccountRequested()
            contentItem: Text {
                text: addButton.text
                color: sidebar.theme.accent
                font.pointSize: sidebar.theme.small
                verticalAlignment: Text.AlignVCenter
                leftPadding: 8
            }
            background: Rectangle {
                radius: 8
                color: addButton.hovered ? sidebar.theme.hover : "transparent"
                border.width: addButton.visualFocus ? 2 : 0
                border.color: sidebar.theme.accent
            }
        }
    }
}
