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

Ubuntu keeps MailHerd's sign-ins in its keychain (GNOME Keyring), the same place your Wi-Fi
passwords live. Your mail is stored in `~/.local/share/mailherd`, encrypted.

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
