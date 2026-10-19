mod bridge;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    // A consistent look on every desktop; the QML draws its own messaging-style UI.
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        // SAFETY: called before any other thread exists.
        unsafe { std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Basic") };
    }

    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("LanternPost"));
        app.as_mut().set_organization_name(&QString::from("LanternPost"));
        app.as_mut().set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
    }

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/LanternPost/qml/Main.qml"));
    }

    let code = app.as_mut().map(|app| app.exec()).unwrap_or(1);
    std::process::exit(code);
}
