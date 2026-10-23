# Installing a test build on Ubuntu

Every change pushed to GitHub builds a fresh installable package automatically.

## Download

1. Open https://github.com/CodeCinder/mailherd/actions and sign in.
2. Click the newest run with a green tick.
3. Scroll to **Artifacts** at the bottom and download **mailherd-deb**.
   **screenshots** shows what that build looks like, if you want to check before installing.
4. The download is a `.zip`. Double-click it to extract `mailherd_….deb`.

## Install

Open a terminal in the folder with the `.deb` and run:

```
sudo apt install ./mailherd_*.deb
```

`apt` also installs the Qt libraries MailHerd needs. Then open **MailHerd** from the app grid.
To update, download a newer build and run the same command.

## First run

MailHerd opens on a welcome screen. Add your accounts there:

- **Gmail** and **Microsoft 365** open your browser to sign in. When the browser says you can
  close the tab, go back to MailHerd.
- **iCloud Mail** asks for your iCloud address and an app-specific password (not your Apple
  Account password). Make one at https://account.apple.com under **Sign-In and Security →
  App-Specific Passwords**.

The first sync brings in the last 90 days of mail, newest first. Each account shows its sync
status under its name in the sidebar. Right-click an account to check for mail now, sign in
again or remove it.

MailHerd keeps its sign-ins in the system keychain: GNOME Keyring on Ubuntu, KDE Wallet on
Kubuntu. After a restart, KDE Wallet may ask for its password the first time MailHerd opens.
Your mail is stored in `~/.local/share/mailherd`, encrypted with a key from that keychain.

If MailHerd ever says the key for its saved mail is missing or doesn't match, choose
**Start fresh**. Your mail is safe on the servers: MailHerd deletes its copy on this computer,
then you add your accounts again and the mail downloads again.

To look around without your accounts, choose **explore the demo** on the welcome screen.

## Remove

```
sudo apt remove mailherd
```

This leaves your stored mail in place. To delete it as well, remove the accounts in MailHerd
first (right-click each account, then **Remove from MailHerd**).

## Reporting a problem

Say what you did, what you expected and what happened. A screenshot helps. If the app won't
start, run `mailherd` in a terminal and copy what it prints.

# Installing a test build on a Mac

MailHerd for Mac runs on Intel and Apple Silicon Macs with macOS 13 (Ventura) or newer.

## Download and install

1. Open https://github.com/CodeCinder/mailherd/actions, click the newest run with a green
   tick, and download **mailherd-mac** under **Artifacts** (**mac-screenshots** shows the build).
2. Double-click the downloaded `.zip`, then the `MailHerd-….dmg` inside it.
3. Drag **MailHerd** onto **Applications**.

## The first time you open it

Until MailHerd is signed with an Apple Developer account, macOS doesn't know who made it and
blocks it the first time:

1. Open MailHerd from Applications. macOS says it can't verify the app; click **Done** (or **OK**).
2. Open **System Settings → Privacy & Security**, scroll down to the message about MailHerd and
   click **Open Anyway**, then confirm with your Mac password.

After that it opens normally. On older macOS versions, right-click MailHerd and choose **Open**
instead.

Your sign-ins are kept in the macOS keychain. Each new test build may ask once for permission
to use them: click **Always Allow**. Your mail is stored in
`~/Library/Application Support/MailHerd`, encrypted.

## Updating and removing

To update, install a newer build the same way and replace the old one. To remove MailHerd,
remove your accounts in the app first (right-click each account, then **Remove from
MailHerd**), then drag MailHerd from Applications to the Bin.
