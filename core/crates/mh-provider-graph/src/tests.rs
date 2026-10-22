use super::*;
use mh_auth::testing::MockApi;

const MIME: &str = "Message-ID: <m1@contoso.example>\r\nFrom: Grace <grace@contoso.example>\r\nTo: me@contoso.example\r\nSubject: Roadmap\r\n\r\nHi\r\n";

fn graph(api: &MockApi) -> Graph {
    Graph::with_base("w1".into(), Box::new(|| Ok("tok".to_string())), &api.url)
}

fn folders(api: &MockApi) {
    api.on(
        "GET",
        "/me/mailFolders/inbox",
        200,
        r#"{"id":"F-IN","displayName":"Inbox"}"#,
    );
    api.on(
        "GET",
        "/me/mailFolders/sentitems",
        200,
        r#"{"id":"F-SENT","displayName":"Sent Items"}"#,
    );
    api.on(
        "GET",
        "/me/mailFolders/archive",
        404,
        r#"{"error":{"code":"ErrorFolderNotFound"}}"#,
    );
    api.on(
        "GET",
        "/me/mailFolders/drafts",
        200,
        r#"{"id":"F-DR","displayName":"Drafts"}"#,
    );
    api.on(
        "GET",
        "/me/mailFolders/deleteditems",
        200,
        r#"{"id":"F-DEL","displayName":"Deleted Items"}"#,
    );
    api.on(
        "GET",
        "/me/mailFolders/junkemail",
        200,
        r#"{"id":"F-JUNK","displayName":"Junk Email"}"#,
    );
}

fn run(g: &mut Graph, cursor: Option<&str>) -> (Result<String, ProviderError>, Vec<SyncEvent>) {
    let mut events = Vec::new();
    let r = g.sync(cursor, &mut |e| events.push(e));
    (r, events)
}

#[test]
fn first_sync_downloads_mime_from_inbox_and_sent() {
    let api = MockApi::start();
    folders(&api);
    let next = format!(
        "{}/me/mailFolders/F-IN/messages/delta?$skiptoken=2",
        api.url
    );
    let page1 = format!(
        r#"{{"value":[{{"id":"A","conversationId":"C1","isRead":false,"receivedDateTime":"2026-10-19T10:00:00Z"}}],"@odata.nextLink":"{next}"}}"#
    );
    api.on_query("GET", "/me/mailFolders/F-IN/messages/delta", "skiptoken=2", 200,
        r#"{"value":[{"id":"B","conversationId":"C1","isRead":true,"flag":{"flagStatus":"flagged"},"receivedDateTime":"2026-10-19T11:00:00Z"}],"@odata.deltaLink":"https://graph.example/delta-in-1"}"#);
    api.on(
        "GET",
        "/me/mailFolders/F-IN/messages/delta",
        200,
        Box::leak(page1.into_boxed_str()),
    );
    api.on(
        "GET",
        "/me/mailFolders/F-SENT/messages/delta",
        200,
        r#"{"value":[],"@odata.deltaLink":"https://graph.example/delta-sent-1"}"#,
    );
    api.on("GET", "/me/messages/A/$value", 200, MIME);
    api.on("GET", "/me/messages/B/$value", 200, MIME);

    let mut g = graph(&api);
    let (cursor, events) = run(&mut g, None);
    let cursor: Cursor = serde_json::from_str(&cursor.expect("synced")).expect("cursor");
    assert_eq!(cursor.delta["F-IN"], "https://graph.example/delta-in-1");
    assert_eq!(cursor.delta["F-SENT"], "https://graph.example/delta-sent-1");
    assert!(cursor.known.contains("A") && cursor.known.contains("B"));

    let first = &api.requests_to("GET", "/me/mailFolders/F-IN/messages/delta")[0];
    assert!(
        first.target.contains("receivedDateTime"),
        "{}",
        first.target
    );
    assert!(
        first
            .headers
            .iter()
            .any(|h| h.to_lowercase().starts_with("prefer:") && h.contains("ImmutableId"))
    );

    let messages: Vec<(String, Vec<String>, Flags, Option<String>)> = events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::Message {
                provider_id,
                folders,
                flags,
                provider_thread_id,
                ..
            } => Some((
                provider_id.clone(),
                folders.clone(),
                *flags,
                provider_thread_id.clone(),
            )),
            _ => None,
        })
        .collect();
    // Newest first.
    assert_eq!(messages[0].0, "B");
    assert!(messages[0].2.seen && messages[0].2.flagged);
    assert_eq!(messages[1].0, "A");
    assert_eq!(messages[1].1, vec!["F-IN"]);
    assert_eq!(messages[1].3.as_deref(), Some("C1"));
    let folder_ids: Vec<String> = events
        .iter()
        .find_map(|e| match e {
            SyncEvent::Folders { folders, .. } => {
                Some(folders.iter().map(|f| f.provider_id.clone()).collect())
            }
            _ => None,
        })
        .expect("folders");
    assert_eq!(
        folder_ids,
        vec!["F-IN", "F-SENT", "F-DR", "F-DEL", "F-JUNK"]
    );
}

#[test]
fn later_syncs_update_known_mail_detect_moves_and_deletions() {
    let api = MockApi::start();
    api.on(
        "GET",
        "/me/mailFolders/inbox",
        200,
        r#"{"id":"F-IN","displayName":"Inbox"}"#,
    );
    api.on("GET", "/me/mailFolders/sentitems", 404, "{}");
    api.on(
        "GET",
        "/me/mailFolders/archive",
        200,
        r#"{"id":"F-AR","displayName":"Archive"}"#,
    );
    for f in ["drafts", "deleteditems", "junkemail"] {
        api.on("GET", &format!("/me/mailFolders/{f}"), 404, "{}");
    }
    let in_link = format!("{}/delta-in", api.url);
    let ar_link = format!("{}/delta-ar", api.url);
    // A was read; M moved to Archive; D deleted; N is new.
    api.on(
        "GET",
        "/delta-in",
        200,
        r#"{"value":[
        {"id":"A","isRead":true,"receivedDateTime":"2026-10-19T10:00:00Z"},
        {"id":"M","@removed":{"reason":"changed"}},
        {"id":"D","@removed":{"reason":"deleted"}},
        {"id":"N","isRead":false,"receivedDateTime":"2026-10-20T09:00:00Z"}
    ],"@odata.deltaLink":"https://graph.example/in-2"}"#,
    );
    api.on("GET", "/delta-ar", 200, r#"{"value":[{"id":"M","isRead":true,"receivedDateTime":"2026-10-18T10:00:00Z"}],"@odata.deltaLink":"https://graph.example/ar-2"}"#);
    api.on("GET", "/me/messages/N/$value", 200, MIME);

    let cursor = json!({ "delta": { "F-IN": in_link, "F-AR": ar_link }, "known": ["A", "M", "D"] })
        .to_string();
    let (result, events) = run(&mut graph(&api), Some(&cursor));
    let next: Cursor = serde_json::from_str(&result.expect("synced")).expect("cursor");
    assert!(next.known.contains("N") && !next.known.contains("D"));

    assert!(events.iter().any(|e| matches!(e, SyncEvent::State { provider_id, flags: Some(f), .. } if provider_id == "A" && f.seen)));
    assert!(events.iter().any(|e| matches!(e, SyncEvent::State { provider_id, folders: Some(f), .. } if provider_id == "M" && f == &vec!["F-AR".to_string()])));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SyncEvent::Removed { provider_id, .. } if provider_id == "M")),
        "a move is not a deletion"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SyncEvent::Removed { provider_id, .. } if provider_id == "D"))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SyncEvent::Message { provider_id, .. } if provider_id == "N"))
    );
    assert!(
        api.requests_to("GET", "/me/messages/A/$value").is_empty(),
        "known mail isn't downloaded again"
    );
}

#[test]
fn sends_mime_and_applies_actions() {
    let api = MockApi::start();
    api.on("POST", "/me/sendMail", 202, "");
    api.on("PATCH", "/me/messages/", 200, "{}");
    api.on("POST", "/me/messages/", 201, "{}");
    let mut g = graph(&api);
    g.send(MIME.as_bytes()).expect("sent");
    let sent = &api.requests_to("POST", "/me/sendMail")[0];
    assert_eq!(
        STANDARD.decode(sent.body.trim()).expect("b64"),
        MIME.as_bytes()
    );
    assert!(
        sent.headers
            .iter()
            .any(|h| h.eq_ignore_ascii_case("content-type: text/plain"))
    );

    g.apply(&RemoteAction::MarkRead {
        provider_ids: vec!["A=1".into()],
    })
    .expect("read");
    g.apply(&RemoteAction::Archive {
        provider_ids: vec!["B".into(), "local:3".into()],
    })
    .expect("archived");
    let patch = &api.requests_to("PATCH", "/me/messages/")[0];
    assert_eq!(patch.target, "/me/messages/A%3D1");
    let body: serde_json::Value = serde_json::from_str(&patch.body).expect("json");
    assert_eq!(body["isRead"], true);
    let moves = api.requests_to("POST", "/me/messages/");
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].target, "/me/messages/B/move");
    let body: serde_json::Value = serde_json::from_str(&moves[0].body).expect("json");
    assert_eq!(body["destinationId"], "archive");
}

#[test]
fn email_address_prefers_mail_then_upn() {
    let api = MockApi::start();
    api.once(
        "GET",
        "/me",
        200,
        r#"{"mail":"me@contoso.example","userPrincipalName":"upn@contoso.example"}"#,
    );
    api.once(
        "GET",
        "/me",
        200,
        r#"{"mail":null,"userPrincipalName":"upn@contoso.example"}"#,
    );
    let mut g = graph(&api);
    assert_eq!(g.email_address().expect("addr"), "me@contoso.example");
    assert_eq!(g.email_address().expect("addr"), "upn@contoso.example");
}
