import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// "Sending in 7s · Undo", shown while a message waits in its undo window.
Rectangle {
    id: bar

    property Theme theme
    property var pending: []
    signal undo(int outboxId)

    property real now: Date.now() / 1000
    readonly property var latest: pending.length > 0 ? pending[pending.length - 1] : null
    readonly property int secondsLeft: latest ? Math.max(0, Math.ceil(latest.send_at - now)) : 0

    function refresh() { now = Date.now() / 1000 }

    visible: latest !== null && secondsLeft > 0
    radius: 20
    color: theme.dark ? "#e9eaee" : "#1f2329"
    implicitWidth: row.implicitWidth + 32
    implicitHeight: row.implicitHeight + 16
    Accessible.role: Accessible.AlertMessage
    Accessible.name: label.text

    RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: 16

        Text {
            id: label
            text: qsTr("Sending in %1s").arg(bar.secondsLeft)
            color: bar.theme.dark ? "#1a1c20" : "#ffffff"
            font.pointSize: bar.theme.small
        }

        AbstractButton {
            id: undoButton
            implicitWidth: undoText.implicitWidth
            implicitHeight: undoText.implicitHeight
            Accessible.name: qsTr("Undo send")
            onClicked: if (bar.latest) bar.undo(bar.latest.outbox_id)
            contentItem: Text {
                id: undoText
                text: qsTr("Undo")
                color: "#8ab4f8"
                font.pointSize: bar.theme.small
                font.bold: true
                font.underline: undoButton.visualFocus
            }
        }
    }
}
