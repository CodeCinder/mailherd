use super::*;
use mh_auth::testing::MockApi;

const RAW_1: &str = "Message-ID: <m1@x>\r\nFrom: Ada <ada@x.example>\r\nTo: me@gmail.example\r\nSubject: Hi\r\n\r\nHello\r\n";

/// Provider id, folders, flags, raw bytes and received time of a message event.
type Downloaded = (String, Vec<String>, Flags, Vec<u8>, i64);

fn gmail(api: &MockApi) -> Gmail {
    Gmail::with_base("g1".into(), Box::new(|| Ok("tok".to_string())), &api.url)
}

fn collect(
    provider: &mut Gmail,
    cursor: Option<&str>,
) -> (Result<String, ProviderError>, Vec<SyncEvent>) {
    let mut events = Vec::new();
    let result = provider.sync(cursor, &mut |e| events.push(e));
    (result, events)
}

fn labels_route(api: &MockApi) {
    api.on(
        "GET",
        "/labels",
        200,
        r#"{"labels":[{"id":"INBOX","name":"INBOX","type":"system"},{"id":"SENT","name":"SENT","type":"system"},
            {"id":"UNREAD","name":"UNREAD","type":"system"},{"id":"CATEGORY_PROMOTIONS","name":"CATEGORY_PROMOTIONS","type":"system"},
            {"id":"Label_7","name":"Receipts","type":"user"}]}"#,
    );
}

fn raw_message(id: &str, labels: &str) -> String {
    format!(
        r#"{{"id":"{id}","threadId":"t1","labelIds":{labels},"internalDate":"1792411200000","raw":"{}"}}"#,
        B64.encode(RAW_1)
    )
}

#[test]
fn first_sync_downloads_recent_mail_and_maps_labels() {
    let api = MockApi::start();
    labels_route(&api);
    api.on(
        "GET",
        "/profile",
        200,
        r#"{"emailAddress":"me@gmail.example","historyId":"500"}"#,
    );
    api.on_query(
        "GET",
        "/messages",
        "maxResults=",
        200,
        r#"{"messages":[{"id":"a","threadId":"t1"},{"id":"b","threadId":"t2"}]}"#,
    );
    api.on(
        "GET",
        "/messages/a",
        200,
        &raw_message("a", r#"["INBOX","UNREAD","CATEGORY_PERSONAL"]"#),
    );
    api.on(
        "GET",
        "/messages/b",
        200,
        &raw_message("b", r#"["Label_7"]"#),
    );

    let mut g = gmail(&api);
    let (cursor, events) = collect(&mut g, None);
    assert_eq!(cursor.expect("synced"), r#"{"history_id":"500"}"#);

    let folders = events.iter().find_map(|e| match e {
        SyncEvent::Folders { folders, .. } => Some(folders.clone()),
        _ => None,
    });
    let ids: Vec<String> = folders
        .expect("folders")
        .into_iter()
        .map(|f| f.provider_id)
        .collect();
    assert_eq!(ids, vec!["INBOX", "SENT", "Label_7", ARCHIVE_FOLDER]);

    let messages: Vec<Downloaded> = events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::Message {
                provider_id,
                folders,
                flags,
                raw,
                received_at,
                ..
            } => Some((
                provider_id.clone(),
                folders.clone(),
                *flags,
                raw.clone(),
                *received_at,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, vec!["INBOX"]);
    assert!(!messages[0].2.seen);
    assert_eq!(messages[0].3, RAW_1.as_bytes());
    assert_eq!(messages[0].4, 1_792_411_200);
    // A label but no Inbox: archived.
    assert_eq!(messages[1].1, vec!["Label_7", ARCHIVE_FOLDER]);
    assert!(messages[1].2.seen);

    let query = &api.requests_to("GET", "/messages?")[0].target;
    assert!(query.contains("newer_than%3A90d"), "{query}");
    assert!(matches!(
        events.last(),
        Some(SyncEvent::Status {
            status: SyncStatus::UpToDate { .. },
            ..
        })
    ));
}

#[test]
fn incremental_sync_applies_history() {
    let api = MockApi::start();
    labels_route(&api);
    api.on(
        "GET",
        "/history",
        200,
        r#"{"historyId":"620","history":[
            {"messagesAdded":[{"message":{"id":"new1","labelIds":["INBOX","UNREAD"]}}]},
            {"messagesAdded":[{"message":{"id":"junk","labelIds":["SPAM"]}}]},
            {"labelsRemoved":[{"message":{"id":"old1","labelIds":["INBOX"]},"labelIds":["UNREAD"]}]},
            {"labelsRemoved":[{"message":{"id":"old2","labelIds":[]},"labelIds":["INBOX"]}]},
            {"messagesDeleted":[{"message":{"id":"gone"}}]}
        ]}"#,
    );
    api.on(
        "GET",
        "/messages/new1",
        200,
        &raw_message("new1", r#"["INBOX","UNREAD"]"#),
    );

    let mut g = gmail(&api);
    let (cursor, events) = collect(&mut g, Some(r#"{"history_id":"500"}"#));
    assert_eq!(cursor.expect("synced"), r#"{"history_id":"620"}"#);
    assert!(api.requests_to("GET", "/history?startHistoryId=500").len() == 1);
    assert!(
        api.requests_to("GET", "/messages/junk").is_empty(),
        "spam is not downloaded"
    );

    let states: Vec<(String, Option<Vec<String>>, bool)> = events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::State {
                provider_id,
                folders,
                flags,
                ..
            } => Some((
                provider_id.clone(),
                folders.clone(),
                flags.is_some_and(|f| f.seen),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(states[0], ("old1".into(), Some(vec!["INBOX".into()]), true));
    assert_eq!(
        states[1],
        ("old2".into(), Some(vec![ARCHIVE_FOLDER.into()]), true)
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SyncEvent::Removed { provider_id, .. } if provider_id == "gone"))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SyncEvent::Message { provider_id, .. } if provider_id == "new1"))
    );
}

#[test]
fn expired_history_falls_back_to_a_full_sync() {
    let api = MockApi::start();
    labels_route(&api);
    api.on("GET", "/history", 404, r#"{"error":{"code":404}}"#);
    api.on(
        "GET",
        "/profile",
        200,
        r#"{"emailAddress":"me@gmail.example","historyId":"900"}"#,
    );
    api.on_query("GET", "/messages", "maxResults=", 200, r#"{"messages":[]}"#);
    let (cursor, _) = collect(&mut gmail(&api), Some(r#"{"history_id":"1"}"#));
    assert_eq!(cursor.expect("synced"), r#"{"history_id":"900"}"#);
}

#[test]
fn rejected_tokens_ask_for_sign_in() {
    let api = MockApi::start();
    api.on("GET", "/labels", 401, "{}");
    let (result, _) = collect(&mut gmail(&api), None);
    assert_eq!(result, Err(ProviderError::NeedsSignIn));
}

#[test]
fn sends_raw_mail_and_applies_actions() {
    let api = MockApi::start();
    api.on("POST", "/messages/send", 200, r#"{"id":"s1"}"#);
    api.on("POST", "/messages/batchModify", 204, "");
    let mut g = gmail(&api);
    g.send(RAW_1.as_bytes()).expect("sent");
    let sent: serde_json::Value =
        serde_json::from_str(&api.requests_to("POST", "/messages/send")[0].body).expect("json");
    let decoded = B64.decode(sent["raw"].as_str().expect("raw")).expect("b64");
    assert_eq!(decoded, RAW_1.as_bytes());

    g.apply(&RemoteAction::Archive {
        provider_ids: vec!["a".into(), "local:5".into()],
    })
    .expect("archived");
    g.apply(&RemoteAction::MarkRead {
        provider_ids: vec!["local:9".into()],
    })
    .expect("nothing to do");
    let modifies = api.requests_to("POST", "/messages/batchModify");
    assert_eq!(modifies.len(), 1, "local-only copies are skipped");
    let body: serde_json::Value = serde_json::from_str(&modifies[0].body).expect("json");
    assert_eq!(body["ids"], json!(["a"]));
    assert_eq!(body["removeLabelIds"], json!(["INBOX"]));
}
