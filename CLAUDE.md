# MailHerd — notes for Claude

The owner is not a programmer. Claude writes and maintains all code; the owner installs builds and reports what they see.

- `docs/SPEC.md` is the blueprint. Read the relevant section before changing behaviour, and keep it in sync with the living spec doc linked at its top.
- All mail logic lives in the Rust core (`core/`). UIs only render view models from `mh-app` and send actions back. Never put sync, threading, quote stripping or provider logic in a UI.
- Email is hostile input: parse only in Rust, never `unwrap()` on data from a message or a server, and sanitise HTML before any UI sees it.
- Every core change comes with tests. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` in `core/` before committing.
- Never commit credentials, OAuth client secrets or real mail. Test messages in `testdata/` are synthetic.
- Explain results to the owner in plain language: what changed, what they can try, and what isn't done yet.
