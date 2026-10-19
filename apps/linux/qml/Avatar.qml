import QtQuick

// A round avatar with initials.
Rectangle {
    id: avatar

    property string initials: ""
    property color tint: "#888888"
    property int size: 36

    width: size
    height: size
    radius: size / 2
    color: tint
    Accessible.ignored: true

    Text {
        anchors.centerIn: parent
        text: avatar.initials
        color: "#ffffff"
        font.pixelSize: avatar.size * 0.4
        font.bold: true
    }
}
