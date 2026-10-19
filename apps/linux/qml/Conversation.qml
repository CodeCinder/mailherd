import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Right pane: the selected conversation as a chat, with the reply box below.
Rectangle {
    id: pane

    property Theme theme
    property var conversation: null
    property var pending: []
    property var htmlLookup: function (id) { return "" }
    readonly property alias composer: composer
    signal pinRequested()
    signal doneRequested()
    signal send(string text)
    signal undo(int outboxId)

    color: theme.window

    // Message HTML is fetched on demand and kept while the conversation is open.
    property var htmlCache: ({})
    onConversationChanged: {
        if (!conversation || conversation.id !== lastId) {
            htmlCache = {}
            lastId = conversation ? conversation.id : ""
            Qt.callLater(function () { view.positionViewAtEnd() })
        }
    }
    property string lastId: ""

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: pane.conversation !== null

        // Header: title, people, Pin and Done.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: header.implicitHeight + 20
            color: pane.theme.window
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: pane.theme.border
            }

            RowLayout {
                id: header
                anchors.fill: parent
                anchors.margins: 10
                anchors.leftMargin: 18
                anchors.rightMargin: 12
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Text {
                        text: pane.conversation ? pane.conversation.title : ""
                        color: pane.theme.text
                        font.pointSize: pane.theme.base * 1.1
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Text {
                        text: pane.conversation ? pane.conversation.subtitle : ""
                        color: pane.theme.subtext
                        font.pointSize: pane.theme.small
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }

                ToolButton {
                    text: pane.conversation && pane.conversation.pinned ? "★" : "☆"
                    font.pointSize: pane.theme.title
                    Accessible.name: pane.conversation && pane.conversation.pinned ? qsTr("Unpin (P)") : qsTr("Pin (P)")
                    ToolTip.visible: hovered
                    ToolTip.text: Accessible.name
                    onClicked: pane.pinRequested()
                    background: Rectangle {
                        radius: 8
                        color: parent.hovered || parent.visualFocus ? pane.theme.hover : "transparent"
                    }
                }
                ToolButton {
                    text: "✓"
                    font.pointSize: pane.theme.title
                    Accessible.name: qsTr("Done (E)")
                    ToolTip.visible: hovered
                    ToolTip.text: Accessible.name
                    onClicked: pane.doneRequested()
                    background: Rectangle {
                        radius: 8
                        color: parent.hovered || parent.visualFocus ? pane.theme.hover : "transparent"
                    }
                }
            }
        }

        ListView {
            id: view
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 6
            topMargin: 12
            bottomMargin: 16
            leftMargin: 16
            rightMargin: 16
            model: pane.conversation ? pane.conversation.items : []
            ScrollBar.vertical: ScrollBar {}
            Accessible.role: Accessible.List
            Accessible.name: qsTr("Messages")
            onCountChanged: Qt.callLater(function () { view.positionViewAtEnd() })

            delegate: Loader {
                id: itemLoader
                required property var modelData
                width: ListView.view.width - 32
                sourceComponent: modelData.type === "day" ? dayComponent : bubbleComponent

                Component {
                    id: dayComponent
                    Item {
                        implicitHeight: 32
                        Rectangle {
                            anchors.centerIn: parent
                            radius: 10
                            color: pane.theme.bubbleOther
                            implicitWidth: dayText.implicitWidth + 20
                            implicitHeight: dayText.implicitHeight + 6
                            Text {
                                id: dayText
                                anchors.centerIn: parent
                                text: itemLoader.modelData.label
                                color: pane.theme.subtext
                                font.pointSize: pane.theme.tiny
                                font.bold: true
                            }
                        }
                    }
                }
                Component {
                    id: bubbleComponent
                    Bubble {
                        theme: pane.theme
                        message: itemLoader.modelData
                        isGroup: pane.conversation ? pane.conversation.is_group : false
                        html: pane.htmlCache[itemLoader.modelData.message_id] || ""
                        onHtmlRequested: function (id) {
                            var cache = pane.htmlCache
                            cache[id] = pane.htmlLookup(id)
                            pane.htmlCache = cache
                            pane.htmlCacheChanged()
                        }
                    }
                }
            }
        }

        Composer {
            id: composer
            theme: pane.theme
            replyLabel: pane.conversation ? pane.conversation.reply_label : ""
            Layout.fillWidth: true
            onSend: function (text) { pane.send(text) }
        }
    }

    UndoBar {
        theme: pane.theme
        pending: pane.pending
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: composer.height + 16
        onUndo: function (id) { pane.undo(id) }
        Timer {
            interval: 250
            running: parent.visible || pane.pending.length > 0
            repeat: true
            onTriggered: parent.refresh()
        }
    }

    Column {
        anchors.centerIn: parent
        visible: pane.conversation === null
        spacing: 8
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "✉"
            font.pointSize: pane.theme.base * 3
            color: pane.theme.border
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Select a conversation")
            color: pane.theme.faint
            font.pointSize: pane.theme.base
        }
    }
}
