import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First run: no accounts yet. Add one, or look around the demo first.
Rectangle {
    id: welcome

    property Theme theme
    property var signIn: null
    // Set when MailHerd couldn't open its database (usually the keychain).
    property string startupError: ""
    // The saved mail can't be unlocked; starting fresh is the way out.
    property bool canStartFresh: false

    signal start(string provider, string email, string password)
    signal dismiss()
    signal demoRequested()
    signal retryRequested()
    signal startFreshRequested()

    color: theme.sidebar

    Flickable {
        anchors.fill: parent
        contentHeight: Math.max(height, card.implicitHeight + 80)
        clip: true

        Rectangle {
            id: card
            width: Math.min(460, welcome.width - 48)
            implicitHeight: column.implicitHeight + 56
            anchors.horizontalCenter: parent.horizontalCenter
            y: Math.max(40, (parent.height - height) / 2)
            radius: 16
            color: welcome.theme.card
            border.color: welcome.theme.border

            ColumnLayout {
                id: column
                anchors.fill: parent
                anchors.margins: 28
                spacing: 16

                Text {
                    text: qsTr("Welcome to MailHerd")
                    color: welcome.theme.text
                    font.pointSize: welcome.theme.title * 1.3
                    font.bold: true
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: welcome.theme.subtext
                    font.pointSize: welcome.theme.base
                    text: welcome.startupError === ""
                        ? qsTr("Add your email accounts to get started. Your mail is kept on this "
                            + "computer, encrypted, and your passwords stay in the system keychain.")
                        : qsTr("MailHerd couldn't start.")
                }

                Rectangle {
                    visible: welcome.startupError !== ""
                    Layout.fillWidth: true
                    radius: 8
                    color: Qt.rgba(0.85, 0.19, 0.15, 0.1)
                    implicitHeight: errorText.implicitHeight + 20
                    Text {
                        id: errorText
                        anchors.fill: parent
                        anchors.margins: 10
                        wrapMode: Text.WordWrap
                        color: welcome.theme.text
                        font.pointSize: welcome.theme.base
                        text: welcome.startupError + "\n\n" + (welcome.canStartFresh
                            ? qsTr("Your mail is safe on the servers. Start fresh deletes MailHerd's copy "
                                + "on this computer; then add your accounts again and your mail downloads "
                                + "again. Replies still waiting to be sent are lost.")
                            : qsTr("MailHerd keeps its passwords in the system keychain (KDE Wallet on "
                                + "Kubuntu, GNOME Keyring on Ubuntu). Make sure it's turned on and "
                                + "unlocked, then try again."))
                    }
                }
                RowLayout {
                    visible: welcome.startupError !== ""
                    Layout.fillWidth: true
                    Button {
                        text: qsTr("Try again")
                        onClicked: welcome.retryRequested()
                    }
                    Item { Layout.fillWidth: true }
                    Button {
                        visible: welcome.canStartFresh
                        text: qsTr("Start fresh")
                        onClicked: welcome.startFreshRequested()
                    }
                }

                AddAccount {
                    visible: welcome.startupError === ""
                    theme: welcome.theme
                    signIn: welcome.signIn
                    Layout.fillWidth: true
                    onStart: function (provider, email, password) { welcome.start(provider, email, password) }
                    onDismiss: welcome.dismiss()
                }

                Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: welcome.theme.border }

                Button {
                    id: demo
                    flat: true
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("Or explore the demo with sample conversations")
                    contentItem: Text {
                        text: demo.text
                        color: welcome.theme.accent
                        font.pointSize: welcome.theme.small
                    }
                    background: Item {}
                    onClicked: welcome.demoRequested()
                }
            }
        }
    }
}
