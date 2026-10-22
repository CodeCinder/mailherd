use super::*;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;

use mh_app::sync::RemoteFolder;
use mh_app::{Core, RemoteAction, Work};
use mh_auth::{Credential, MemoryStore, Provider};
use mh_model::Flags;
use mh_store::{FolderRole, Store};

type Pass = Result<Vec<SyncEvent>, ProviderError>;

#[derive(Default)]
struct Script {
    /// What each sync pass returns, per account. When empty, passes succeed
    /// with nothing new.
    passes: HashMap<AccountId, VecDeque<Pass>>,
    /// The cursor each pass started from.
    cursors: Vec<(AccountId, Option<String>)>,
    sent: Vec<(AccountId, Vec<u8>)>,
    applied: Vec<(AccountId, RemoteAction)>,
    job_error: Option<ProviderError>,
    renewed: Vec<AccountId>,
    whoami: String,
    password_ok: bool,
    oauth: Option<OAuthClient>,
}

struct Fake {
    script: Arc<Mutex<Script>>,
    vault: Vault,
}

struct FakeProvider {
    account: AccountId,
    script: Arc<Mutex<Script>>,
}

impl MailProvider for FakeProvider {
    fn sync(
        &mut self,
        cursor: Option<&str>,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<String, ProviderError> {
        let (pass, n) = {
            let mut s = self.script.lock().expect("lock");
            s.cursors
                .push((self.account.clone(), cursor.map(String::from)));
            let n = s.cursors.len();
            (
                s.passes.get_mut(&self.account).and_then(|q| q.pop_front()),
                n,
            )
        };
        for event in pass.transpose()?.unwrap_or_default() {
            emit(event);
        }
        Ok(format!("{}-{n}", self.account))
    }

    fn send(&mut self, raw: &[u8]) -> Result<(), ProviderError> {
        let mut s = self.script.lock().expect("lock");
        if let Some(e) = s.job_error.clone() {
            return Err(e);
        }
        s.sent.push((self.account.clone(), raw.to_vec()));
        Ok(())
    }

    fn apply(&mut self, action: &RemoteAction) -> Result<(), ProviderError> {
        let mut s = self.script.lock().expect("lock");
        if let Some(e) = s.job_error.clone() {
            return Err(e);
        }
        s.applied.push((self.account.clone(), action.clone()));
        Ok(())
    }
}

impl Connector for Fake {
    fn connect(&self, account: &Account) -> Result<Box<dyn MailProvider>, ProviderError> {
        Ok(Box::new(FakeProvider {
            account: account.id.clone(),
            script: Arc::clone(&self.script),
        }))
    }
    fn whoami(&self, _: ProviderKind, token: &str) -> Result<String, ProviderError> {
        assert_eq!(token, "at", "uses the token from the sign-in");
        Ok(self.script.lock().expect("lock").whoami.clone())
    }
    fn check_password(&self, _: &str, password: &str) -> Result<(), ProviderError> {
        if self.script.lock().expect("lock").password_ok && password == "abcd-efgh-ijkl-mnop" {
            Ok(())
        } else {
            Err(ProviderError::NeedsSignIn)
        }
    }
    fn oauth_client(&self, _: ProviderKind) -> Option<OAuthClient> {
        self.script.lock().expect("lock").oauth.clone()
    }
    fn vault(&self) -> &Vault {
        &self.vault
    }
    fn renew(&self, account: &Account) {
        self.script
            .lock()
            .expect("lock")
            .renewed
            .push(account.id.clone());
    }
}

fn fast() -> Timing {
    Timing {
        poll: Duration::from_secs(60),
        poll_imap: Duration::from_secs(60),
        retry: Duration::from_millis(20),
        retry_max: Duration::from_millis(40),
        after_job: Duration::from_secs(60),
        sign_in_timeout: Duration::from_secs(10),
    }
}

struct Harness {
    engine: Engine,
    transport: SyncTransport,
    script: Arc<Mutex<Script>>,
    vault: Vault,
    seen: Vec<SyncEvent>,
}

fn harness() -> Harness {
    let script = Arc::new(Mutex::new(Script::default()));
    let vault = Vault::new(Arc::new(MemoryStore::default()));
    let fake = Fake {
        script: Arc::clone(&script),
        vault: vault.clone(),
    };
    let (engine, transport) = Engine::new(Arc::new(fake), fast());
    Harness {
        engine,
        transport,
        script,
        vault,
        seen: Vec::new(),
    }
}

impl Harness {
    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().expect("lock")
    }

    /// Poll the engine until `done` holds for everything seen so far.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        loop {
            let events = self.engine.poll();
            self.seen.extend(events);
            if done(self) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn cursor_events(&self, account: &str) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                SyncEvent::Cursor { account_id, cursor } if account_id == account => {
                    Some(cursor.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn statuses(&self, account: &str) -> Vec<SyncStatus> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                SyncEvent::Status { account_id, status } if account_id == account => {
                    Some(status.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn results(&mut self, count: usize) -> Vec<JobResult> {
        let start = Instant::now();
        let mut results = Vec::new();
        while results.len() < count {
            results.extend(self.transport.poll());
            assert!(start.elapsed() < Duration::from_secs(10), "no job results");
            std::thread::sleep(Duration::from_millis(5));
        }
        results.sort_by_key(|r| r.id);
        results
    }
}

fn account(id: &str, kind: ProviderKind) -> Account {
    Account {
        id: id.into(),
        kind,
        email: format!("{id}@example.org"),
        display_name: "You".into(),
        color: 0,
    }
}

fn job(id: i64, account_id: &str, work: Work) -> OutboxJob {
    OutboxJob {
        id,
        account: account(account_id, ProviderKind::Gmail),
        work,
    }
}

#[test]
fn syncs_on_start_and_carries_the_cursor_forward() {
    let mut h = harness();
    h.script().passes.insert(
        "a".into(),
        VecDeque::from([Ok(vec![SyncEvent::Folders {
            account_id: "a".into(),
            folders: vec![],
        }])]),
    );
    h.engine
        .start_account(&account("a", ProviderKind::Gmail), Some("saved".into()));
    h.until("first pass", |h| h.cursor_events("a").len() == 1);
    assert!(matches!(h.seen[0], SyncEvent::Folders { .. }));
    assert_eq!(h.cursor_events("a"), vec!["a-1"]);

    h.engine.sync_now(Some("a"));
    h.until("second pass", |h| h.cursor_events("a").len() == 2);
    let cursors: Vec<Option<String>> = h.script().cursors.iter().map(|c| c.1.clone()).collect();
    assert_eq!(cursors, vec![Some("saved".into()), Some("a-1".into())]);
}

#[test]
fn outbox_jobs_reach_the_right_account() {
    let mut h = harness();
    h.engine
        .start_account(&account("a", ProviderKind::Gmail), None);
    h.engine
        .start_account(&account("b", ProviderKind::ICloud), None);
    let archive = RemoteAction::Archive {
        provider_ids: vec!["m1".into()],
    };
    h.transport
        .submit(job(1, "a", Work::Apply(archive.clone())));
    h.transport.submit(job(
        2,
        "b",
        Work::Send {
            raw: b"Subject: hi\r\n\r\nhello".to_vec(),
        },
    ));
    h.transport
        .submit(job(3, "nobody", Work::Apply(archive.clone())));

    let results = h.results(3);
    assert_eq!(results[0].result, Ok(()));
    assert_eq!(results[1].result, Ok(()));
    assert!(results[2].result.is_err(), "unknown accounts fail at once");
    let s = h.script();
    assert_eq!(s.applied, vec![("a".into(), archive)]);
    assert_eq!(s.sent[0].0, "b");
}

#[test]
fn failed_jobs_report_why() {
    let mut h = harness();
    h.script().job_error = Some(ProviderError::Retry("HTTP 503".into()));
    h.engine
        .start_account(&account("a", ProviderKind::Gmail), None);
    h.transport
        .submit(job(7, "a", Work::Send { raw: b"x".to_vec() }));
    assert_eq!(h.results(1)[0].result, Err("HTTP 503".into()));
}

#[test]
fn a_rejected_sign_in_waits_for_the_user() {
    let mut h = harness();
    h.script().passes.insert(
        "a".into(),
        VecDeque::from([
            Err(ProviderError::NeedsSignIn),
            Err(ProviderError::NeedsSignIn),
        ]),
    );
    h.engine
        .start_account(&account("a", ProviderKind::Gmail), None);
    h.until("asks to sign in", |h| {
        h.statuses("a").contains(&SyncStatus::NeedsSignIn)
    });
    assert_eq!(h.script().renewed, vec!["a"], "tried a fresh token first");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(h.script().cursors.len(), 2, "stops trying on its own");

    h.engine.sync_now(Some("a"));
    h.until("syncs after signing in", |h| {
        h.cursor_events("a").len() == 1
    });
}

#[test]
fn temporary_errors_are_retried() {
    let mut h = harness();
    h.script().passes.insert(
        "a".into(),
        VecDeque::from([
            Err(ProviderError::Retry("offline".into())),
            Err(ProviderError::Retry("offline".into())),
        ]),
    );
    h.engine
        .start_account(&account("a", ProviderKind::Microsoft365), None);
    h.until("recovers", |h| h.cursor_events("a").len() == 1);
    assert_eq!(
        h.statuses("a"),
        vec![
            SyncStatus::Error {
                message: "offline".into()
            };
            2
        ]
    );
}

#[test]
fn removing_an_account_stops_it_and_forgets_the_sign_in() {
    let mut h = harness();
    let creds = Credential::Password {
        username: "a@example.org".into(),
        password: "pw".into(),
    };
    h.vault.save("a", &creds).expect("saved");
    h.engine
        .start_account(&account("a", ProviderKind::ICloud), None);
    assert!(h.engine.is_running("a"));
    h.engine.remove_account("a").expect("removed");
    assert!(!h.engine.is_running("a"));
    assert_eq!(h.vault.credential("a"), Err(VaultError::Missing));
    h.transport
        .submit(job(1, "a", Work::Send { raw: b"x".to_vec() }));
    assert!(h.results(1)[0].result.is_err());
}

fn icloud(email: &str) -> NewAccount {
    NewAccount::ICloud {
        email: email.into(),
        password: " abcd-efgh-ijkl-mnop ".into(),
    }
}

fn added(h: &Harness) -> Vec<Account> {
    h.seen
        .iter()
        .filter_map(|e| match e {
            SyncEvent::AccountAdded { account } => Some(account.clone()),
            _ => None,
        })
        .collect()
}

fn finished(h: &Harness) -> bool {
    matches!(
        h.engine.sign_in_state(),
        Some(SignInState::Done { .. } | SignInState::Failed { .. })
    )
}

#[test]
fn icloud_sign_in_checks_and_saves_the_password() {
    let mut h = harness();
    h.script().password_ok = true;
    assert_eq!(
        h.engine.sign_in(icloud(" me@icloud.example "), &[]),
        &SignInState::Checking
    );
    h.until("signed in", finished);
    assert_eq!(
        h.engine.sign_in_state(),
        Some(&SignInState::Done {
            email: "me@icloud.example".into()
        })
    );
    let accounts = added(&h);
    assert_eq!(accounts.len(), 1);
    let a = &accounts[0];
    assert_eq!(a.kind, ProviderKind::ICloud);
    assert_eq!(a.email, "me@icloud.example");
    assert_eq!(
        h.vault.credential(&a.id),
        Ok(Credential::Password {
            username: "me@icloud.example".into(),
            password: "abcd-efgh-ijkl-mnop".into(),
        })
    );
    let id = a.id.clone();
    h.until("new account syncs", |h| h.cursor_events(&id).len() == 1);

    h.engine.dismiss_sign_in();
    assert_eq!(h.engine.sign_in_state(), None);
}

#[test]
fn a_wrong_icloud_password_is_explained() {
    let mut h = harness();
    h.engine.sign_in(icloud("me@icloud.example"), &[]);
    h.until("checked", finished);
    let Some(SignInState::Failed { message }) = h.engine.sign_in_state() else {
        panic!("should fail");
    };
    assert!(message.contains("app-specific password"), "{message}");
    assert!(added(&h).is_empty());
}

#[test]
fn signing_in_again_reuses_the_account() {
    let mut h = harness();
    h.script().password_ok = true;
    let mut existing = account("old", ProviderKind::ICloud);
    existing.email = "Me@iCloud.example".into();
    h.script().passes.insert(
        "old".into(),
        VecDeque::from([
            Err(ProviderError::NeedsSignIn),
            Err(ProviderError::NeedsSignIn),
        ]),
    );
    h.engine.start_account(&existing, None);
    h.until("asks to sign in", |h| {
        h.statuses("old").contains(&SyncStatus::NeedsSignIn)
    });

    h.engine
        .sign_in(icloud("me@icloud.example"), std::slice::from_ref(&existing));
    h.until("signed in", finished);
    assert!(added(&h).is_empty(), "no duplicate account");
    assert!(h.vault.credential("old").is_ok());
    h.until("resyncs with the new password", |h| {
        h.cursor_events("old").len() == 1
    });
}

#[test]
fn sign_in_without_a_registration_says_so() {
    let mut h = harness();
    let Some(SignInState::Failed { message }) =
        Some(h.engine.sign_in(NewAccount::Google, &[]).clone())
    else {
        panic!("should fail");
    };
    assert!(
        message.contains("isn't set up to sign in to Google"),
        "{message}"
    );
}

fn query_value(url: &str, key: &str) -> String {
    url::Url::parse(url)
        .expect("url")
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .expect("query value")
}

/// Act as the browser coming back from the provider's sign-in page.
fn browser_returns(redirect_uri: &str, query: &str) {
    let addr = redirect_uri
        .replace("http://localhost", "127.0.0.1")
        .replace("http://", "");
    let mut s = TcpStream::connect(addr).expect("connect");
    write!(s, "GET /?{query} HTTP/1.1\r\nHost: x\r\n\r\n").expect("write");
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
}

#[test]
fn browser_sign_in_adds_the_account() {
    let mut h = harness();
    let tokens = mh_auth::testing::serve(vec![(
        200,
        r#"{"access_token":"at","refresh_token":"rt","expires_in":3599}"#,
    )]);
    h.script().whoami = "me@gmail.example".into();
    h.script().oauth = Some(OAuthClient {
        provider: Provider::Google,
        client_id: "cid".into(),
        client_secret: Some("sec".into()),
        auth_url: "https://accounts.example/auth".into(),
        token_url: format!("{}/token", tokens.url),
        scopes: vec!["mail".into()],
        redirect_host: "127.0.0.1",
        extra_auth_params: vec![],
    });

    let SignInState::Browser { url } = h.engine.sign_in(NewAccount::Google, &[]).clone() else {
        panic!("should open the browser");
    };
    assert!(url.starts_with("https://accounts.example/auth?"));
    browser_returns(
        &query_value(&url, "redirect_uri"),
        &format!("code=abc&state={}", query_value(&url, "state")),
    );
    h.until("signed in", finished);
    assert_eq!(
        h.engine.sign_in_state(),
        Some(&SignInState::Done {
            email: "me@gmail.example".into()
        })
    );
    let a = added(&h).pop().expect("account added");
    assert_eq!(a.kind, ProviderKind::Gmail);
    let Ok(Credential::OAuth { tokens }) = h.vault.credential(&a.id) else {
        panic!("tokens saved");
    };
    assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
}

#[test]
fn closing_the_sheet_abandons_the_sign_in() {
    let mut h = harness();
    h.script().password_ok = true;
    h.engine.sign_in(icloud("me@icloud.example"), &[]);
    h.engine.dismiss_sign_in();
    std::thread::sleep(Duration::from_millis(100));
    h.until("drained", |_| true);
    assert!(added(&h).is_empty());
    assert_eq!(h.engine.sign_in_state(), None);
}

/// The whole loop: sign in, mail syncs into the core, a reply goes out.
#[test]
fn works_end_to_end_with_the_core() {
    let script = Arc::new(Mutex::new(Script {
        password_ok: true,
        ..Script::default()
    }));
    let vault = Vault::new(Arc::new(MemoryStore::default()));
    let (mut engine, transport) = Engine::new(
        Arc::new(Fake {
            script: Arc::clone(&script),
            vault,
        }),
        Timing {
            after_job: Duration::from_millis(10),
            ..fast()
        },
    );
    let mut core = Core::new(
        Store::open_in_memory().expect("store"),
        Box::new(transport),
        Box::new(|| 1_792_411_200),
        0,
    )
    .expect("core");
    core.set_undo_seconds(0);

    engine.sign_in(icloud("me@icloud.example"), core.accounts());
    let start = Instant::now();
    while core.accounts().is_empty() {
        core.apply_sync(engine.poll()).expect("applied");
        assert!(start.elapsed() < Duration::from_secs(10), "no account");
        std::thread::sleep(Duration::from_millis(5));
    }
    let id = core.accounts()[0].id.clone();

    // The next pass brings in a message.
    script.lock().expect("lock").passes.insert(
        id.clone(),
        VecDeque::from([Ok(vec![
            SyncEvent::Folders {
                account_id: id.clone(),
                folders: vec![
                    RemoteFolder {
                        provider_id: "INBOX".into(),
                        name: "Inbox".into(),
                        role: Some(FolderRole::Inbox),
                    },
                    RemoteFolder {
                        provider_id: "Sent".into(),
                        name: "Sent".into(),
                        role: Some(FolderRole::Sent),
                    },
                ],
            },
            SyncEvent::Message {
                account_id: id.clone(),
                provider_id: "1:1:INBOX".into(),
                provider_thread_id: None,
                raw: b"Message-ID: <q@x>\r\nFrom: Ada <ada@x.example>\r\nTo: me@icloud.example\r\n\
                       Subject: Lunch?\r\nDate: Mon, 19 Oct 2026 12:00:00 +0000\r\n\r\nThursday?\r\n"
                    .to_vec(),
                folders: vec!["INBOX".into()],
                flags: Flags::default(),
                received_at: 1_792_411_000,
            },
        ])]),
    );
    engine.sync_now(Some(&id));
    let start = Instant::now();
    while core.thread_list(&mh_app::view::Mailbox::Inbox).is_empty() {
        core.apply_sync(engine.poll()).expect("applied");
        assert!(start.elapsed() < Duration::from_secs(10), "no mail");
        std::thread::sleep(Duration::from_millis(5));
    }
    let thread = core.thread_list(&mh_app::view::Mailbox::Inbox)[0]
        .thread_id
        .clone();
    core.send_reply(&thread, "Yes!").expect("queued");

    let start = Instant::now();
    while script.lock().expect("lock").sent.is_empty() {
        core.process_outbox().expect("outbox");
        core.apply_sync(engine.poll()).expect("applied");
        assert!(start.elapsed() < Duration::from_secs(10), "not sent");
        std::thread::sleep(Duration::from_millis(5));
    }
    let raw = String::from_utf8(script.lock().expect("lock").sent[0].1.clone()).expect("utf8");
    assert!(raw.contains("To: Ada <ada@x.example>") || raw.contains("ada@x.example"));
    assert!(raw.contains("Yes!"));
}

mod session {
    use super::*;
    use crate::Session;

    fn fake() -> (Arc<Mutex<Script>>, Arc<dyn Connector>) {
        let script = Arc::new(Mutex::new(Script {
            password_ok: true,
            ..Script::default()
        }));
        let connector = Arc::new(Fake {
            script: Arc::clone(&script),
            vault: Vault::new(Arc::new(MemoryStore::default())),
        });
        (script, connector)
    }

    fn tick_until(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) {
        let start = Instant::now();
        while !done(session) {
            session.tick().expect("tick");
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn saved_accounts_resume_where_they_left_off() {
        let (script, connector) = fake();
        let store = Store::open_in_memory().expect("store");
        store
            .upsert_account(&account("a", ProviderKind::Gmail))
            .expect("account");
        store.set_sync_state("a", "c9").expect("cursor");
        let mut session = Session::start(store, connector, fast(), 0).expect("session");
        assert!(!session.is_demo());
        tick_until(&mut session, "cursor saved", |s| {
            s.core().store().sync_state("a").ok().flatten().as_deref() == Some("a-1")
        });
        assert_eq!(
            script.lock().expect("lock").cursors[0],
            ("a".into(), Some("c9".into()))
        );
    }

    #[test]
    fn adding_and_removing_an_account() {
        let (_, connector) = fake();
        let mut session = Session::start(
            Store::open_in_memory().expect("store"),
            connector,
            fast(),
            0,
        )
        .expect("session");
        assert_eq!(
            session.sign_in(icloud("me@icloud.example")),
            Some(SignInState::Checking)
        );
        tick_until(&mut session, "account added", |s| {
            !s.core().accounts().is_empty()
        });
        assert!(matches!(
            session.sign_in_state(),
            Some(SignInState::Done { .. })
        ));
        session.dismiss_sign_in();
        assert_eq!(session.sign_in_state(), None);

        let id = session.core().accounts()[0].id.clone();
        session.remove_account(&id).expect("removed");
        assert!(session.core().accounts().is_empty());
        assert_eq!(session.core().sidebar().accounts.len(), 0);
    }

    #[test]
    fn the_demo_stays_offline() {
        let mut session = Session::demo(0).expect("demo");
        assert!(session.is_demo());
        assert!(!session.core().accounts().is_empty());
        assert_eq!(session.sign_in(NewAccount::Google), None);
        session.tick().expect("tick");
    }

    #[test]
    fn data_lives_in_the_users_data_folder() {
        let dir = crate::default_data_dir().expect("data dir");
        assert!(dir.is_absolute());
        assert!(dir.ends_with("mailherd") || dir.ends_with("MailHerd"));
    }
}
