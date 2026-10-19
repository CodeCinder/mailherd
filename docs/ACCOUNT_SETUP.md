# Account setup (one time)

Before MailHerd can sign in to Gmail and Microsoft 365, each provider needs to know the
app exists. You register it once with Google and once with Microsoft, then store the IDs
they give you as GitHub secrets so the builds can use them. iCloud needs only an
app-specific password, which you type into MailHerd itself.

Allow about 30 minutes. Nothing here costs money.

**Keep these values out of chats, emails and files.** Paste them only where these steps
say: GitHub secrets, or MailHerd's own sign-in screen.

---

## 1. Google (Gmail)

1. Go to <https://console.cloud.google.com> and sign in with your Gmail account.
2. **Create a project.** Click the project picker at the top left → **New project** →
   name it `MailHerd` → **Create**. Make sure `MailHerd` is selected in the picker afterwards.
3. **Turn on the APIs.** Menu (☰) → **APIs & Services** → **Library**. Search for each of
   these, open it and click **Enable**:
   - Gmail API
   - People API
   - Google Calendar API
   - Google Drive API
4. **Set up the sign-in screen.** Menu (☰) → **Google Auth platform** → **Branding** →
   **Get started**:
   - App name: `MailHerd`. User support email: your Gmail address. → **Next**
   - Audience: **External** → **Next**
   - Contact information: your Gmail address → **Next**
   - Tick the agreement → **Continue** → **Create**
5. **Add yourself as a tester.** **Audience** → under **Test users** click **Add users** →
   enter your Gmail address → **Save**.
6. **Add the permissions.** **Data Access** → **Add or Remove Scopes**. In
   **Manually add scopes**, paste this whole line, then click **Add to table** → **Update** → **Save**:
   ```
   https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/gmail.settings.basic https://www.googleapis.com/auth/contacts.readonly https://www.googleapis.com/auth/calendar https://www.googleapis.com/auth/drive.file
   ```
7. **Create the app's ID.** **Clients** → **Create Client** → Application type **Desktop app**
   → name `MailHerd Linux` → **Create**. A box shows a **Client ID** and a **Client secret**.
   Keep this tab open for step 4 below.

Note: while the app is in "Testing", Google asks you to sign in again every 7 days. Once
everything works, we'll switch it to "In production" (still private to you) to stop that.
When you sign in, Google will warn that it "hasn't verified this app". That's expected,
because you are the developer. Choose **Advanced** → **Go to MailHerd**.

---

## 2. Microsoft 365

You need to be an admin of your Microsoft 365 organisation, which you are.

1. Go to <https://entra.microsoft.com> and sign in with your work admin account.
2. **Entra ID** → **App registrations** → **New registration**:
   - Name: `MailHerd`
   - Supported account types: **Single tenant only** (your organisation)
   - Redirect URI: choose **Public client/native (mobile & desktop)** and enter `http://localhost`
   - **Register**
3. On the **Overview** page that opens, note the **Application (client) ID** and the
   **Directory (tenant) ID**.
4. **Authentication** → under **Advanced settings**, set **Allow public client flows** to
   **Yes** → **Save**.
5. **API permissions** → **Add a permission** → **Microsoft Graph** → **Delegated permissions**.
   Tick each of these (use the search box), then **Add permissions**:
   - `offline_access`, `openid`, `profile`, `email`, `User.Read`
   - `Mail.ReadWrite`, `Mail.Send`, `MailboxSettings.ReadWrite`
   - `Contacts.Read`, `Calendars.ReadWrite`, `Files.ReadWrite`
6. Still on **API permissions**, click **Grant admin consent for (your organisation)** → **Yes**.
   Each permission should now show a green tick and "Granted".

There's no secret to copy for Microsoft: the two IDs from step 3 are all MailHerd needs.

---

## 3. iCloud

iCloud doesn't let other apps use your Apple ID password, so you create a separate password
just for MailHerd. Do this when the MailHerd build that can sign in arrives, since you'll type
it straight into the app.

1. Go to <https://account.apple.com> and sign in. Two-factor authentication must be on, which
   it is for almost every Apple ID.
2. **Sign-In and Security** → **App-Specific Passwords** → **+** (or **Generate an app-specific password**).
3. Name it `MailHerd` → **Create**. Apple shows a password like `abcd-efgh-ijkl-mnop`.
4. Type it into MailHerd's iCloud sign-in screen. You can revoke it on the same page at any time,
   and that won't affect your Apple ID.

MailHerd signs in with your **@icloud.com** address, which must have iCloud Mail turned on.

---

## 4. Store the IDs in GitHub

The builds read the Google and Microsoft IDs from GitHub's encrypted secrets, so they are never
saved in the code.

1. Go to <https://github.com/prophetizer/lanternpost> → **Settings** → **Secrets and variables**
   → **Actions** → **New repository secret**.
2. Add these four secrets, one at a time. The **Name** must match exactly:

   | Name | Value |
   | --- | --- |
   | `GOOGLE_CLIENT_ID` | Client ID from Google step 7 (ends in `.apps.googleusercontent.com`) |
   | `GOOGLE_CLIENT_SECRET` | Client secret from Google step 7 |
   | `MS_CLIENT_ID` | Application (client) ID from Microsoft step 3 |
   | `MS_TENANT_ID` | Directory (tenant) ID from Microsoft step 3 |

3. Tell Claude when they're in. Claude can see that the secrets exist, but never their values.
