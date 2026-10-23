import AppKit
import SwiftUI
import WebKit

/// Shows an email's HTML, already sanitised by the core. Scripts are off and
/// nothing is fetched from the internet (no tracking pixels or remote
/// images); clicked links open in the browser.
struct HTMLView: NSViewRepresentable {
    let html: String

    /// Blocks every network load: the page shows only what the core sent.
    private static let blockRemote = """
    [{"trigger":{"url-filter":"^(https?|ftp|wss?)://"},"action":{"type":"block"}}]
    """

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.setValue(false, forKey: "drawsBackground")
        let html = self.html
        WKContentRuleListStore.default().compileContentRuleList(
            forIdentifier: "MailHerdBlockRemote", encodedContentRuleList: Self.blockRemote
        ) { list, _ in
            // Without the block list, show nothing rather than risk loading remote content.
            guard let list else { return }
            view.configuration.userContentController.add(list)
            view.loadHTMLString(html, baseURL: nil)
        }
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {}

    final class Coordinator: NSObject, WKNavigationDelegate {
        func webView(
            _ webView: WKWebView,
            decidePolicyFor action: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            if action.navigationType == .linkActivated, let url = action.request.url {
                NSWorkspace.shared.open(url)
                decisionHandler(.cancel)
            } else {
                decisionHandler(.allow)
            }
        }
    }
}
