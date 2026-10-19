# LanternPost

A native, cross-platform email client that makes email feel like messaging. It works with iCloud, Gmail and Microsoft 365.

- Conversations shown as chat bubbles, with quoted history and signatures tucked away
- A combined inbox, plus a People view that groups everything by who you're talking to
- Normal, standards-compliant email going out, so recipients never need LanternPost
- Private by default: remote images blocked, trackers stripped, local data encrypted

**Status:** early development, building towards v0.1 (Linux). Nothing is ready to use yet.

## Platforms

| Platform | UI | Status |
| --- | --- | --- |
| Linux (Ubuntu 26.04) | Qt 6 / QML | In progress (v0.1) |
| macOS | SwiftUI | Planned |
| Windows | WinUI 3 | Planned |
| iOS / iPadOS | SwiftUI | Planned |
| Android | Jetpack Compose | Planned |

All platforms share one Rust core (`core/`) that owns sync, threading, the chat view, storage and search.

## Documentation

- [Architecture and product spec](docs/SPEC.md)

## Licence

Proprietary. Copyright © 2026 Michael Guglielmo. All rights reserved. See [LICENSE](LICENSE).
