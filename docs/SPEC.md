# MailHerd — Architecture & Product Spec

As of 22 Oct 2026. This is a copy of the living spec document; the doc is the source of truth for
decisions, and this file is updated whenever the doc changes:
https://claude.ai/code/artifact/6deb31a5-a09b-47ad-9adb-ebe997ab38ce

## Overview

MailHerd is a native, cross-platform email client that makes email feel like messaging. It supports iCloud, Gmail and Microsoft 365, and shows conversations as chat bubbles while still sending normal, standards-compliant email.

- **Audience:** personal use by the owner first. Every decision keeps a later commercial release possible (Google and Microsoft app verification, app stores, a private licence).
- **Platforms, in order:** Linux (Ubuntu 26.04 LTS) → macOS → Windows → iOS / iPadOS → Android.
- **Who builds it:** Claude writes and maintains all code. The owner tests builds, reports issues, and does the one-time account setup.

**Guiding principles**

1. One shared Rust core holds all mail logic, so behaviour is identical on every platform and is tested automatically.
2. Each UI is native and thin: it renders state from the core and sends user actions back to it.
3. Mail stays between your devices and your providers. The home backend never stores mail, with one exception: encrypted scheduled sends.
4. Private by default: remote images blocked, trackers stripped, local data encrypted.
5. Outgoing mail is always normal email. Recipients never need MailHerd.

## Decisions log

These are the decisions from the planning interview of 19 Oct 2026.

| Area | Decision | Notes |
| --- | --- | --- |
| Platforms | Linux, macOS, Windows, iOS/iPadOS, Android | Linux first, macOS second |
| Sync model | Hybrid | Devices sync directly with providers; the backend only handles push, send-later and webhooks |
| Code structure | Shared Rust core plus native UIs | Chosen after reviewing Rust, .NET, C++, Kotlin Multiplatform and separate apps |
| Core scope | The core owns view state | UIs render ready-made models and send actions back |
| Linux UI | Qt 6 / QML, with Rust glue | Connected through cxx-qt. Planned as C++ glue; written in Rust instead so there is one less language to maintain |
| macOS / iOS UI | SwiftUI | Rust connected through UniFFI |
| Windows UI | WinUI 3 / C# | Rust connected through uniffi-bindgen-cs |
| Android UI | Kotlin / Jetpack Compose | Rust connected through UniFFI |
| Builds | Private GitHub repo with GitHub Actions | Installable .deb first, then .dmg, .msix and mobile builds |
| Owner's OS | Ubuntu 26.04 LTS | |
| Accounts | Personal iCloud, personal Gmail, Microsoft 365 work tenant (owner is admin) | |
| Chat model | Threads view and People view, switchable | Each thread can show as bubbles or as classic email |
| Quotes | Strip quoted history and signatures, with an expander | Detection is tested against a test corpus |
| Outgoing mail | Normal email | Correct threading headers, HTML plus plain text, quoted history for recipients who aren't on MailHerd |
| Group reply | Reply to everyone by default | Recipients can be narrowed, or a reply sent privately to one person |
| Inbox | Combined inbox plus per-account views | Colour accent per account |
| Threads across accounts | Merged into one thread | |
| Labels and folders | Shown the way each provider shows them | Gmail labels, iCloud and Outlook folders, Outlook categories as tags |
| Triage | Snooze, reminders, send later, undo send, smart categories, pin, done | |
| Undo send | 10 seconds | Adjustable |
| Send later | Server-side | Microsoft 365 schedules it natively; for Gmail and iCloud the home backend holds an encrypted copy |
| App-only state | Stored inside the mailbox | Keywords, labels and extended properties, so no backend is needed for it |
| Local storage | Smart cache | Full messages for the last 12 months, headers for older mail |
| AI | Later | The architecture leaves a place for it |
| Compose | Chat box that expands into a full rich-text editor | |
| Backend host | Home Linux mini-PC or Raspberry Pi, in Docker | |
| Remote access | Cloudflare Tunnel | |
| iCloud mobile push | Backend holds an encrypted app-specific password | Needed only once mobile apps exist |
| Privacy | Block remote images, strip trackers, encrypt the local database | |
| Encryption | OpenPGP with Autocrypt first, S/MIME later | |
| Contacts and calendar | Contacts autocomplete, invite RSVP, agenda sidebar, then a full calendar | Full calendar comes in a late phase |
| Layout | Three panes, with the conversation as chat on the right | Agenda sidebar can be toggled |
| Keyboard | Gmail/Superhuman-style single keys plus a Ctrl+K command palette | |
| Notifications | People only | Quick reply and Done from the notification |
| Visual style | Modern messaging look | Follows the system light/dark setting |
| Attachments | Inline previews, files view, large-file links, drag out | Large files go via OneDrive or Google Drive |
| Rules | Provider rules plus app rules | iCloud has no rules API, so its rules run in the app or backend |
| Signatures | Per-account signatures, short chat signature, snippets, synced | |
| Accessibility | Screen reader, keyboard-only, font scaling, translation-ready, RTL | |
| Mobile | Swipe gestures, widgets, share sheet, rich notifications, watch support | |
| v0.1 scope | Core chat email | See the phase plan |
| Sending identities | Main address per account in v0.1 | The data model supports aliases, which come in v0.3 |
| Name and licence | MailHerd; private, all rights reserved | |

## System architecture

Four native apps sit on one shared Rust core. The core talks to each mail provider directly, and a small home relay handles only the jobs that must run while your devices are off.

```
 ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
 │ Linux (v0.1) │  │ macOS · iOS  │  │   Windows    │  │   Android    │
 │ Qt 6/QML+Rust│  │   SwiftUI    │  │ WinUI 3 / C# │  │Kotlin/Compose│
 └──────▲───────┘  └──────▲───────┘  └──────▲───────┘  └──────▲───────┘
     cxx-qt            UniFFI      uniffi-bindgen-cs       UniFFI
 ┌──────▼─────────────────▼─────────────────▼─────────────────▼───────┐
 │ Shared Rust core (mailherd-core)  owns all mail logic and state  │
 │  View models · Sync + outbox · Threads + chat · Store + search      │
 │  MIME + sanitiser · Provider adapters · OAuth + keychain · OpenPGP  │
 └──────▲─────────────────▲─────────────────▲─────────────────▲───────┘
   direct sync            │                 │         Cloudflare Tunnel
 ┌──────▼──────┐  ┌───────▼──────┐  ┌───────▼──────┐  ┌───────▼────────┐
 │   iCloud    │  │    Gmail     │  │ Microsoft 365│◄►│Home relay(v0.2)│
 │ IMAP + SMTP │  │Gmail REST API│  │Microsoft Grph│  │Docker, mini-PC │
 │App password │  │ Google OAuth │  │Entra ID OAuth│  │Send-later,rules│
 └─────────────┘  └──────────────┘  └──────────────┘  │Mobile push(ltr)│
                                                      └────────────────┘
```

User actions go down into the core and live view models come back up, through a generated binding layer for each language. The desktop apps work fully without the relay. It adds scheduled sends for Gmail and iCloud, rules that run while devices are off, and, later, mobile push.

## Providers and protocols

Each provider is reached through its best-supported interface, behind one common provider interface in the core.

| | iCloud | Gmail | Microsoft 365 |
| --- | --- | --- | --- |
| Mail access | IMAP (IDLE, CONDSTORE/QRESYNC, MOVE, UIDPLUS) | Gmail REST API v1 | Microsoft Graph v1.0 |
| Sending | SMTP (submission, port 587) | Gmail API `messages.send` | Graph `sendMail` / draft send |
| Sign-in | App-specific password | OAuth 2.0 with PKCE (installed-app flow) | OAuth 2.0 with PKCE (Entra ID public client) |
| Incremental sync | MODSEQ / QRESYNC | `history.list` from a historyId | Delta queries per folder |
| Real-time, desktop | IMAP IDLE from the app (v0.1 polls every 2 min) | Polling `history.list` (every 60 s in v0.1), or Pub/Sub through the backend | Delta polling (every 60 s in v0.1), or webhooks through the backend |
| Real-time, mobile | Backend IMAP IDLE → APNs/FCM | Pub/Sub → backend → APNs/FCM | Graph webhooks → backend → APNs/FCM |
| Thread ID | Computed from headers (JWZ algorithm) | Native `threadId` | Native `conversationId` |
| Labels / folders | Folders | Labels | Folders plus categories |
| App state storage | IMAP keywords (`$MH_Pinned` and so on) plus a hidden `MailHerd` folder | Hidden labels (`MailHerd/Pinned` and so on) | Single-value extended properties plus categories |
| Scheduled send | Backend (encrypted) | Backend (encrypted) | Native deferred send (`PidTagDeferredSendTime`) |
| Server rules | None (no API) | Gmail filters API | Inbox rules API (`messageRules`) |
| Contacts | CardDAV | People API | Graph contacts and people |
| Calendar | CalDAV | Calendar API | Graph calendar |
| Large files | (uses OneDrive or Google Drive) | Google Drive API | OneDrive via Graph |

**Scopes requested.** Gmail: `gmail.modify`, `gmail.settings.basic` (filters), `contacts.readonly`, `calendar`, `drive.file`. Microsoft: `Mail.ReadWrite`, `Mail.Send`, `MailboxSettings.ReadWrite`, `Contacts.Read`, `Calendars.ReadWrite`, `Files.ReadWrite`, `offline_access`.

**Why not IMAP for Gmail and Microsoft 365?** Their APIs give native threads, labels or categories, cheaper incremental sync, push notifications and scheduled send. Exchange Web Services is not used because Microsoft is retiring it in Exchange Online.

**Verification path for a commercial release.** Gmail's `gmail.modify` is a restricted scope, so a public release needs Google OAuth verification plus an annual CASA security assessment. For personal use, the Google project stays in testing mode with the owner as a test user; testing-mode refresh tokens expire after 7 days, so the project should be moved to production unverified (up to 100 users) once it works. Microsoft needs publisher verification for multi-tenant use; in the owner's own tenant, admin consent is enough.

## Core design

The Rust core (`mailherd-core`) is a library that every app embeds. UIs call it to perform actions and subscribe to live view models.

**Crates (modules)**

| Crate | Responsibility | Key libraries |
| --- | --- | --- |
| `mh-model` | Shared types: Account, Identity, Message, Thread, Person, Label | serde |
| `mh-store` | Encrypted SQLite store, schema migrations, blob cache for bodies and attachments | rusqlite + SQLCipher |
| `mh-search` | Full-text index with operators (`from:`, `has:attachment`, `before:`) and server fallback for older mail | tantivy |
| `mh-mime` | Parse and build MIME, decode charsets, sanitise HTML, strip trackers | mail-parser, mail-builder, ammonia |
| `mh-provider-imap` | iCloud IMAP and SMTP | imap, lettre |
| `mh-provider-gmail` | Gmail, People, Calendar and Drive REST APIs | ureq |
| `mh-provider-graph` | Microsoft Graph mail, contacts, calendar and OneDrive | ureq |
| `mh-auth` | OAuth PKCE flows, token refresh, keychain storage, shared HTTP client | keyring, ureq |
| `mh-sync` | Sync engine (one worker thread per account), account sign-in, outbox delivery, app startup (`Session`) | std threads |
| `mh-thread` | Threading, cross-account merge, People grouping | |
| `mh-chat` | Quote and signature stripping, bubble models | |
| `mh-classify` | Smart categories and app-side rules | |
| `mh-crypto` | OpenPGP and Autocrypt, later S/MIME | sequoia-openpgp |
| `mh-app` | App state, view models, action dispatch; the API exposed to UIs | uniffi |

**Data model (main tables).** `accounts`, `identities`, `folders` (also used for labels), `messages` (with RFC Message-ID, provider IDs, flags and keywords), `message_folders` (many-to-many, which is how Gmail labels work), `threads`, `thread_messages`, `people`, `person_addresses`, `attachments`, `app_state` (pinned, done, snoozed and reminder records mirrored to the mailbox), `outbox` (queued actions and sends), `rules`, `signatures`, `snippets`.

**Sync engine**

1. First sync of an account fetches headers newest-first and shows the inbox within seconds. Bodies for the last 12 months download in the background, and older bodies download on demand.
2. Incremental sync uses each provider's change feed (QRESYNC, historyId, delta tokens) and is triggered by IDLE, polling or a push from the backend.
3. Every user action (archive, label, send) is applied to the local store at once, so the UI updates instantly. It is then queued in `outbox` and replayed against the server with retries. If the server rejects an action, the local change is rolled back and you see a notice.
4. Conflicts: the server wins for flags and labels; app state uses the latest timestamp.

**As built in v0.1.** Each account has a background worker that owns its provider connection. Workers send changes (new messages, flag and folder changes, removals, sync cursors, status) to the UI thread, which applies them to the store in one place. The first sync fetches the last 90 days (up to 1,000 messages per folder, newest first). Afterwards Gmail and Microsoft 365 are checked every minute and iCloud every two minutes. Errors are retried with a growing delay (30 s up to 10 min). If a provider rejects the saved sign-in, the worker first tries a fresh access token and then shows "Sign in again" next to the account. Sent mail appears in Sent immediately as a local copy, which is replaced when the provider's copy syncs.

**Sign-in as built.** Google and Microsoft use the system browser with a loopback redirect (`http://127.0.0.1:<port>` for Google, `http://localhost:<port>` for Microsoft) and PKCE. iCloud uses the address and an app-specific password, checked by logging in to IMAP. Tokens and passwords go to the system keychain (Secret Service on Linux). The mail database is encrypted with a random key that is also kept in the keychain. If that key goes missing or stops matching (seen once with KDE Wallet after a restart), MailHerd never makes a new key over the stored mail; it explains the problem and offers Start fresh, which deletes the local copy so the mail downloads again. The app registrations are baked into release builds from GitHub Actions secrets and never committed.

**Threading and merging.** Gmail and Microsoft 365 provide native thread IDs. iCloud threads are computed from `References` and `In-Reply-To` headers, with subject matching as a fallback. Threads from different accounts merge when they share any RFC Message-ID. A message stored in two accounts is shown once, with an indicator of which accounts hold it.

**People view.** Each thread maps to its set of participants, excluding you. One-to-one threads group under that person; group threads group under the participant set, like a group chat. A contact's several email addresses merge into one person, using contacts data plus manual merging.

**Quote and signature stripping.** Removed: quoted blocks (lines starting with `>`, Gmail and Outlook quote containers, "On … wrote:" headers in about 15 languages), forwarded-history blocks, signature delimiters (`-- `), and signatures learned from repeated trailing text per sender. The full original is always one click away. This is tested against a corpus of over 500 real-world samples.

**State stored in the mailbox.** Pinned, Done, snooze-until and reminder are written to the provider (see the providers table) so every device and reinstall sees them. Snooze is implemented by moving the thread out of the inbox and storing the wake time; whichever device or backend sees the time pass moves it back.

**API to the UIs.** The core exposes `dispatch(Action)` and `subscribe(ViewQuery) → stream of ViewModel`. View models include ThreadList, PeopleList, Conversation (bubbles), Composer, Sidebar, Search and Settings. Every UI uses the same models, so behaviour can't drift between platforms.

## Features and UX

The desktop window has three panes: a sidebar, a list, and the conversation as chat. The look is a modern messaging app, following the system light or dark theme.

**Sidebar (left).** Combined Inbox, People, Pinned, Snoozed, Scheduled, Sent, Drafts, then a section per account with its labels or folders, and smart-category streams (People, Newsletters, Notifications, Receipts). Each account has a colour accent.

**List (middle).** In Threads mode: avatar, sender(s), subject, preview of the latest new text (quotes stripped), time, account colour, pin, attachment and unread markers. In People mode: one row per person or group, with the latest message preview, like a messaging app. Pinned items stay on top.

**Conversation (right).**

- Messages shown as bubbles: yours on the right in the accent colour, others' on the left with avatar and name. Group threads show names above bubbles.
- Each bubble shows only new text. A "…" toggle reveals quoted history and signatures, and "View original" opens the full rendered HTML email in a sandboxed viewer.
- Heavy HTML mail (newsletters, receipts) is shown as a card that expands to the full email instead of a bubble.
- A toggle per thread switches between bubble view and classic stacked-email view.
- Images and videos appear inline as photo bubbles; PDFs and documents as cards with thumbnails. Invites appear as cards with Accept, Maybe and Decline.
- Day separators, read state, and a "waiting for reply" marker when a reminder is set.

**Compose**

- A chat box at the bottom of each conversation: Enter sends, Shift+Enter adds a new line. It supports bold, italic, links, lists, emoji, and pasted or dragged attachments.
- The recipient line above the box defaults to everyone in group threads. Click to narrow, or choose "Reply privately" on any bubble.
- An expand button opens the full rich-text editor (fonts, tables, inline images, signature choice, From identity, CC/BCC) as a panel or a separate window.
- New conversation: Ctrl+N or `c`, with recipient autocomplete from all three accounts' contacts.
- Undo send: a 10-second countdown bar after sending. The message is held locally until then.
- Send later: choose a time; scheduled messages appear in Scheduled.
- Outgoing format: multipart/alternative (HTML plus plain text), correct `In-Reply-To` and `References`, and a standard quoted reply below for recipients who aren't on MailHerd.

**Triage**

| Feature | Behaviour | Shortcut |
| --- | --- | --- |
| Done | Archives the thread (Gmail: removes the Inbox label; iCloud/Outlook: moves to Archive) | `e` |
| Pin | Keeps the thread at the top of every list | `p` |
| Snooze | Hides it until a chosen time, then brings it back as unread | `s` |
| Reminder | "Bring this back if nobody replies in N days" | `h` |
| Send later | Schedule a send at compose time | Ctrl+Shift+Enter |
| Smart categories | Automatic streams for People, Newsletters, Notifications and Receipts, using Gmail's own categories where available plus local rules for other accounts | — |

**Rules.** One rule editor. When a rule only moves, labels or marks mail, MailHerd saves it as the provider's own filter (Gmail filters, Outlook inbox rules) so it runs even when no device is on. Other rules (snooze, notify, iCloud anything) run in the app, and on the home backend once it exists.

**Signatures and snippets.** A rich signature per identity, plus an optional one-line chat signature. Signatures are left out of chat replies by default. Snippets are triggered by typing `;name`, with fields like `{first_name}`. Both sync through the hidden MailHerd folder or label in the primary account.

**Attachments.** Inline previews; a Files view per conversation and per person; drag out to save, or Save All. For files over about 20 MB, MailHerd offers to upload to OneDrive or Google Drive and insert a link.

**Notifications.** By default, only messages from real people (smart category People) or pinned and VIP threads notify you. Notifications include Reply, Done and Snooze buttons. Linux uses the freedesktop notification spec through Qt.

**Keyboard.** `j`/`k` to move, `Enter` to open, `e` Done, `p` Pin, `s` Snooze, `r` Reply, `a` Reply all, `f` Forward, `c` Compose, `/` Search, `g i` Go to Inbox, `?` for help, and Ctrl+K for a command palette. Single-key shortcuts can be turned off and remapped.

**Search.** Instant local search as you type, with operators (`from:`, `to:`, `subject:`, `has:attachment`, `label:`, `before:`, `after:`, `account:`). For mail older than the 12-month cache, it falls back to the server.

**Accessibility and languages.** Every action works by keyboard and is labelled for screen readers (Orca on Linux, VoiceOver on Apple, Narrator on Windows, TalkBack on Android). The app respects system font size and high contrast. All text is in translation files (Qt `.ts`, Apple string catalogs, `.resw`, Android `strings.xml`), English first. Layout handles right-to-left text, with direction detected per bubble.

**Mobile (planned).** Swipe gestures (configurable: Done, Snooze, Pin), home-screen widgets (unread from people, pinned chats, agenda), share-sheet target (share files into a conversation), rich notifications with reply, Done and Snooze, and Apple Watch and Wear OS support.

**Contacts and calendar.** v0.2: contacts autocomplete, invite cards with RSVP, and an agenda sidebar. Later phase: a full calendar with day, week and month views, event creation and free/busy lookup.

## Home backend

The backend (`mailherd-relay`) is a small Rust service running in Docker on a home Linux mini-PC or Raspberry Pi. It is reached only through Cloudflare Tunnel, and it arrives in v0.2.

**Jobs**

| Job | Phase | How it works |
| --- | --- | --- |
| Send later (Gmail, iCloud) | v0.2 | The device encrypts the finished message with a key the relay holds only in memory after unlock, uploads it with a send time, and the relay sends it via the Gmail API or SMTP at that time, then deletes it |
| Snooze, reminders and rules when all devices are off | v0.2 | Reads app state from the mailbox and acts at the right time |
| Gmail push | Mobile phase | Google Pub/Sub pull subscription (outbound only) → APNs/FCM |
| Microsoft 365 push | Mobile phase | Graph change-notification webhook at `https://relay.<your-domain>/graph` through the tunnel, with subscription renewal about every 2 days → APNs/FCM |
| iCloud push | Mobile phase | Keeps an IMAP IDLE connection open using the encrypted app-specific password → APNs/FCM |
| Settings backup | Optional | Encrypted backup of local preferences |

**What it stores.** OAuth refresh tokens and the iCloud app password (encrypted at rest with age, key in a root-only file or TPM), scheduled messages (encrypted, deleted after sending), device push tokens, and app state cache. It never stores mailbox contents.

**Access control.** Each device is paired once by scanning a QR code shown by the relay, and gets a device key. All requests are signed. Cloudflare Access protects every path except the Graph webhook, which validates Microsoft's `clientState` secret.

**Push payloads** contain no mail content, only "account X has new mail". The device wakes, syncs directly with the provider, and shows the notification locally.

**Operations.** One `docker compose up -d`, with Watchtower or a systemd timer pulling new images from GitHub Container Registry. Health is reported in the app's settings. Desktop apps work fully without the relay; only send-later for Gmail and iCloud and mobile push depend on it.

## Security and privacy

Email is hostile input, so all parsing happens in memory-safe Rust and all rendering happens in a locked-down viewer.

| Area | Measure |
| --- | --- |
| Credentials | OAuth tokens and the iCloud app password live in the OS keychain (Secret Service / GNOME Keyring or KWallet, macOS/iOS Keychain, Windows Credential Manager, Android Keystore), never in files |
| Local data | SQLCipher-encrypted database and encrypted blob cache; key held in the keychain |
| HTML rendering | Sanitised by ammonia in the core, then shown in Qt WebEngine (WKWebView and WebView2 on other platforms) with JavaScript off, no network access, and a strict content-security policy |
| Remote images | Blocked by default; allow per message or per sender; loading goes through the app so no cookies or referrer are sent |
| Tracking | Known tracking pixels removed; `utm_*`, `fbclid`, `gclid` and similar parameters stripped from links; known redirect wrappers unwrapped |
| Links | The real destination is shown on hover; mismatched link text and target triggers a warning |
| Attachments | Opened with the system default app only after you click; executables trigger a warning |
| Phishing signals | SPF/DKIM/DMARC results from `Authentication-Results` shown on sender; warning for look-alike domains |
| OpenPGP | sequoia-openpgp with Autocrypt headers; keys in the keychain; encrypt automatically when all recipients have keys |
| S/MIME | Later phase; certificates from the OS store |
| Backend | Device pairing, signed requests, Cloudflare Access, encrypted secrets at rest, no mail stored except encrypted scheduled sends |
| Updates | Signed packages; on Linux an apt repository signed with a project GPG key |
| Dependencies | `cargo audit` and `cargo deny` in CI; Dependabot for all ecosystems |

## Build, test and release

One private GitHub monorepo builds everything with GitHub Actions. The owner only downloads and installs packages.

**Repository layout**

```
mailherd/
  core/            Rust workspace (mh-* crates)
  relay/           Rust home backend + Dockerfile
  apps/linux/      Qt 6 / QML on the core via cxx-qt (built with Cargo)
  apps/apple/      SwiftUI (macOS + iOS, Xcode project)
  apps/windows/    WinUI 3 / C# (MSBuild)
  apps/android/    Kotlin / Compose (Gradle)
  testdata/        MIME corpus, quote-stripping samples
  docs/            SPEC.md and setup guides
```

**Testing**

1. Unit tests for each crate, run on every push.
2. Protocol tests against local servers in CI: Dovecot (IMAP), GreenMail (SMTP), plus recorded HTTP fixtures for Gmail and Graph.
3. A corpus of over 500 real-world MIME and quote samples with expected output (golden tests).
4. Fuzzing of the MIME parser and HTML sanitiser with cargo-fuzz.
5. UI smoke tests: Qt Quick Test on Linux, XCUITest on Apple; screenshots attached to each build so the owner can see changes before installing.
6. A manual test checklist for each release, run by the owner against real accounts.

**Release outputs**

| Platform | Package | Distribution |
| --- | --- | --- |
| Linux | `.deb` for Ubuntu 26.04 (amd64, arm64) | GitHub Releases, then a signed apt repository so updates arrive through Software Updater |
| macOS | Signed and notarised `.dmg` | GitHub Releases, Sparkle auto-update |
| Windows | `.msix` | GitHub Releases, then the Microsoft Store |
| iOS | TestFlight build | App Store later |
| Android | `.apk` / `.aab` | Direct install, then Google Play |
| Relay | Docker image (amd64, arm64) | GitHub Container Registry |

**Versioning.** Semantic versions; `main` is always releasable. A nightly channel for the owner, and tagged releases.

## Phase plan

v0.1 is a Linux client you can use every day. Each later phase starts only when the gate before it is passed.

1. **v0.1 · Linux core chat email** — all 3 accounts, combined inbox, Threads and People views, bubbles with quote stripping, chat compose, send and reply, undo send, search, pin, done, archive, privacy defaults, people-only notifications.
   *Gate: you use it as your daily email client on Linux.*
2. **v0.2 · Triage and home relay** — snooze, reminders, send later, smart categories, rules, home relay on the mini-PC, contacts autocomplete, invite RSVP, agenda sidebar, signatures and snippets.
   *Gate: relay stable, scheduled sends reliable.*
3. **v0.3 · Power features** — OpenPGP with Autocrypt, sending aliases, large-file links, Files view, remappable shortcuts.
   *Gate: core feature-complete for desktop.*
4. **macOS · SwiftUI** — feature parity with Linux on the same core; signed and notarised .dmg with auto-update.
   *Gate: parity across two desktops.*
5. **Windows, then iOS and Android** — WinUI 3 app; iOS and Android with relay push, swipes, widgets, share sheet, rich notifications, Apple Watch and Wear OS, S/MIME.
   *Gate: decide on a commercial release.*
6. **Later** — full calendar (day, week, month, event creation), AI features (summaries, smart replies, natural-language search), Google and Microsoft verification for a public release.

No dates are set: progress depends on how quickly each build is tested and reported back.

## Owner setup checklist

**Before v0.1 can sign in (about 30 minutes)**

- [ ] Google Cloud: create a project, enable the Gmail, People, Calendar and Drive APIs, configure the OAuth consent screen, add yourself as a test user, create a Desktop OAuth client ID
- [ ] Microsoft Entra admin center: register an app (public client, redirect `http://localhost`), add the Graph permissions, grant admin consent for your tenant
- [ ] Apple ID: create an app-specific password for MailHerd at account.apple.com
- [ ] Install the first `.deb` on Ubuntu 26.04 and sign in to all three accounts

**Before v0.2 (backend)**

- [ ] Install Ubuntu Server or Raspberry Pi OS and Docker on the mini-PC
- [ ] A domain name on Cloudflare (about $10/year) and a free Cloudflare account
- [ ] Run the provided install script, which sets up the tunnel and the relay container
- [ ] Pair your desktop with the relay by scanning its QR code

**Before macOS and mobile**

- [ ] Apple Developer Program ($99/year) for signing, notarisation, TestFlight and push
- [ ] Firebase project for Android push (free)
- [ ] Google Play developer account ($25 once) when Android ships

## Open questions and risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Scope across five native UIs | Slow progress beyond Linux | Shared core owns all logic and view state; each new UI is mostly rendering |
| Google restricted-scope verification for a commercial release | Months of process plus a yearly CASA assessment fee | Personal use avoids it; revisit only if going commercial |
| Google testing-mode tokens expire after 7 days | Weekly re-sign-in | Publish the Google app as "in production, unverified" for personal use |
| iCloud has no OAuth, push API or rules API | App passwords; IDLE needed for push; iCloud rules run only in the app or relay | Encrypted storage, IDLE on desktop and relay, app-side rules |
| Quote-stripping errors hide real text | Missed content | Conservative detection, always-available "…" expander, golden-test corpus |
| Home server or internet outage | Scheduled sends and mobile push delayed | Desktop works without the relay; scheduled sends retry and you're alerted; Microsoft 365 schedules natively |
| cxx-qt and uniffi-bindgen-cs are younger tools | Binding bugs | Keep the API small and plain (a dispatch and subscribe pattern), with a C ABI fallback |
| The owner can't debug code | Slow feedback loop | Screenshots in CI, an in-app "Report a problem" that bundles anonymised logs, and a clear test checklist |

**Open questions**

- Sending aliases: which addresses (iCloud aliases, Hide My Email, Gmail send-as, M365 shared mailboxes) should v0.3 support first?
- Domain name for the relay and the apt repository: does the owner already own one on Cloudflare?
- The Linux UI is Qt; check how it looks on Ubuntu (GNOME) once v0.1 is installed, and whether it should be tuned toward GNOME's look.
- Full calendar scope (invites to others, shared calendars, time zones), to be settled at that phase.
- AI features (summaries, smart replies, natural-language search): which provider, and what privacy rules, when we get there.
