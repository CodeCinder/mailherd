import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Adding an account: pick a provider, then follow the sign-in. Used on the
// welcome screen and in the "Add account" window. The steps come from the
// core as `signIn` (null until a sign-in starts).
ColumnLayout {
    id: root

    property Theme theme
    property var signIn: null
    // "choose" or "icloud", while no sign-in is running.
    property string page: "choose"

    signal start(string provider, string email, string password)
    signal dismiss()
    signal finished()

    spacing: 14

    readonly property string step: signIn ? signIn.state : page

    // Open the provider's sign-in page in the browser, once per attempt, and
    // only from the copy of this view that is on screen.
    property string openedUrl: ""
    onSignInChanged: {
        if (visible && signIn && signIn.state === "browser" && signIn.url !== openedUrl) {
            openedUrl = signIn.url
            Qt.openUrlExternally(signIn.url)
        }
        if (signIn && signIn.state === "done")
            password.text = ""
    }

    function showICloud(email) {
        page = "icloud"
        if (email)
            emailField.text = email
        password.text = ""
        emailField.forceActiveFocus()
    }

    component ProviderButton: Button {
        id: control
        property color dot
        property string detail
        Layout.fillWidth: true
        implicitHeight: 58
        contentItem: RowLayout {
            spacing: 12
            Rectangle { width: 12; height: 12; radius: 6; color: control.dot }
            ColumnLayout {
                spacing: 1
                Text { text: control.text; color: root.theme.text; font.pointSize: root.theme.base; font.bold: true }
                Text { text: control.detail; color: root.theme.subtext; font.pointSize: root.theme.small }
            }
            Item { Layout.fillWidth: true }
            Text { text: "›"; color: root.theme.faint; font.pointSize: root.theme.title }
        }
        background: Rectangle {
            radius: 10
            color: control.hovered ? root.theme.hover : root.theme.card
            border.width: control.visualFocus ? 2 : 1
            border.color: control.visualFocus ? root.theme.accent : root.theme.border
        }
    }

    component Message: Text {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        color: root.theme.subtext
        font.pointSize: root.theme.base
        lineHeight: 1.15
    }

    component Primary: Button {
        id: primary
        implicitHeight: 36
        contentItem: Text {
            text: primary.text
            color: primary.enabled ? root.theme.accentText : root.theme.subtext
            font.pointSize: root.theme.base
            font.bold: true
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 8
            color: primary.enabled ? (primary.down ? Qt.darker(root.theme.accent, 1.15) : root.theme.accent) : root.theme.border
        }
    }

    component Plain: Button {
        id: plain
        flat: true
        implicitHeight: 36
        contentItem: Text {
            text: plain.text
            color: root.theme.accent
            font.pointSize: root.theme.base
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 8
            color: plain.hovered ? root.theme.hover : "transparent"
            border.width: plain.visualFocus ? 2 : 0
            border.color: root.theme.accent
        }
    }

    // 1. Pick a provider.
    ColumnLayout {
        visible: root.step === "choose"
        Layout.fillWidth: true
        spacing: 8

        ProviderButton {
            text: qsTr("Gmail")
            detail: qsTr("Sign in with Google in your browser")
            dot: "#ea4335"
            onClicked: root.start("google", "", "")
        }
        ProviderButton {
            text: qsTr("Microsoft 365")
            detail: qsTr("Work or school account, in your browser")
            dot: "#0078d4"
            onClicked: root.start("microsoft", "", "")
        }
        ProviderButton {
            text: qsTr("iCloud Mail")
            detail: qsTr("Uses an app-specific password")
            dot: "#34aadc"
            onClicked: root.showICloud("")
        }
    }

    // 2a. iCloud needs an app-specific password.
    ColumnLayout {
        visible: root.step === "icloud"
        Layout.fillWidth: true
        spacing: 10

        Text {
            text: qsTr("Add iCloud Mail")
            color: root.theme.text
            font.pointSize: root.theme.title
            font.bold: true
        }
        Message {
            text: qsTr("Apple doesn't let mail apps use your Apple Account password. Instead, make an "
                + "app-specific password: at account.apple.com, go to Sign-In and Security, then "
                + "App-Specific Passwords, and create one called MailHerd.")
        }
        Plain {
            text: qsTr("Open account.apple.com")
            Layout.alignment: Qt.AlignLeft
            onClicked: Qt.openUrlExternally("https://account.apple.com/account/manage")
        }
        TextField {
            id: emailField
            Layout.fillWidth: true
            placeholderText: qsTr("you@icloud.com")
            inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoAutoUppercase
            Accessible.name: qsTr("iCloud email address")
            onAccepted: password.forceActiveFocus()
        }
        TextField {
            id: password
            Layout.fillWidth: true
            placeholderText: qsTr("App-specific password (xxxx-xxxx-xxxx-xxxx)")
            echoMode: TextInput.Password
            Accessible.name: qsTr("App-specific password")
            onAccepted: if (addICloud.enabled) addICloud.clicked()
        }
        RowLayout {
            Layout.fillWidth: true
            Plain { text: qsTr("Back"); onClicked: root.page = "choose" }
            Item { Layout.fillWidth: true }
            Primary {
                id: addICloud
                text: qsTr("Add account")
                enabled: emailField.text.indexOf("@") > 0 && password.text.length > 0
                onClicked: root.start("icloud", emailField.text, password.text)
            }
        }
    }

    // 2b. Waiting for the browser.
    ColumnLayout {
        visible: root.step === "browser"
        Layout.fillWidth: true
        spacing: 10

        BusyIndicator { running: visible; Layout.alignment: Qt.AlignHCenter }
        Message {
            horizontalAlignment: Text.AlignHCenter
            text: qsTr("Finish signing in in your browser, then come back here.")
        }
        RowLayout {
            Layout.fillWidth: true
            Plain { text: qsTr("Cancel"); onClicked: { root.page = "choose"; root.dismiss() } }
            Item { Layout.fillWidth: true }
            Plain {
                text: qsTr("Open the sign-in page again")
                onClicked: if (root.signIn) Qt.openUrlExternally(root.signIn.url)
            }
        }
    }

    // 2c. Checking the iCloud password.
    ColumnLayout {
        visible: root.step === "checking"
        Layout.fillWidth: true
        spacing: 10
        BusyIndicator { running: visible; Layout.alignment: Qt.AlignHCenter }
        Message {
            horizontalAlignment: Text.AlignHCenter
            text: qsTr("Checking with iCloud…")
        }
    }

    // 3. Done or failed.
    ColumnLayout {
        visible: root.step === "done"
        Layout.fillWidth: true
        spacing: 10
        Text {
            text: "✓"
            color: root.theme.accent
            font.pointSize: root.theme.title * 1.6
            Layout.alignment: Qt.AlignHCenter
        }
        Message {
            horizontalAlignment: Text.AlignHCenter
            text: root.signIn && root.signIn.email
                ? qsTr("Added %1. Your mail is on its way — recent messages come first.").arg(root.signIn.email)
                : ""
        }
        RowLayout {
            Layout.fillWidth: true
            Plain { text: qsTr("Add another account"); onClicked: { root.page = "choose"; root.dismiss() } }
            Item { Layout.fillWidth: true }
            Primary { text: qsTr("Done"); onClicked: { root.page = "choose"; root.dismiss(); root.finished() } }
        }
    }

    ColumnLayout {
        visible: root.step === "failed"
        Layout.fillWidth: true
        spacing: 10
        Rectangle {
            Layout.fillWidth: true
            radius: 8
            color: Qt.rgba(0.85, 0.19, 0.15, 0.1)
            implicitHeight: failText.implicitHeight + 20
            Text {
                id: failText
                anchors.fill: parent
                anchors.margins: 10
                wrapMode: Text.WordWrap
                text: root.signIn && root.signIn.message ? root.signIn.message : ""
                color: root.theme.text
                font.pointSize: root.theme.base
            }
        }
        RowLayout {
            Layout.fillWidth: true
            Plain { text: qsTr("Cancel"); onClicked: { root.page = "choose"; root.dismiss(); root.finished() } }
            Item { Layout.fillWidth: true }
            // Back to the form (iCloud keeps what was typed) or the provider list.
            Primary { text: qsTr("Try again"); onClicked: root.dismiss() }
        }
    }
}
