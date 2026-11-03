# Accounts, teams and cloud files: design

> **Status:** phase 1 begun, 2026-11-03: the server's sign-in page (email and password, Google
> when set up), sessions and `figcraft-server users` are built ([`server.md`](server.md#accounts)).
> The browser build knows who is signed in and shows a new account the onboarding steps and an
> editor tour (`account.*`, `onboarding.*`, `help.tour`). Still to come in phase 1: browser sign-in
> from the desktop app, and the app's account menu. Closes most of gap 2 ("Multiplayer and sharing
> are a preview, without accounts") in [`gaps.md`](gaps.md).

FigCraft's files can live on a `figcraft-server` that the team runs (one VPS is enough), the way
Figma's live in Figma's cloud: people sign in, files belong to a team and sit in folders or in
someone's Drafts, deleted files go to Trash, and the Home screen is a file browser over all of it.
Local files keep working with no server and no account.

Behaviour comes from Figma's public help center, blog and developer docs (sources at the end).
Nothing is taken from Figma's apps or network traffic.

## What the server already does

`figcraft-server` already stores files the way Figma's multiplayer servers do: one live document
per open file in memory, every change appended to a checksummed log before anyone is told, and the
log folded into a new snapshot (compaction) after 1,000 changes, 16 MiB, when the last person
leaves, or on shutdown ([`server.md`](server.md#the-data-directory)). Figma's equivalent is a
checkpoint to S3 every 30–60 s plus a journal of sequence-numbered changes. The storage engine
stays as it is. What is missing is everything around it: people, teams, folders, trash, versions
and a file list per person.

## Data model

```text
user ── member of ──► team ──► folder ──► file
  │                     │                  ▲
  └── drafts (per team) ┴──────────────────┘   (a file with no folder is its owner's draft)
```

| Thing | Fields | Notes |
|---|---|---|
| User | id, email, name, avatar colour, password hash (argon2id), created, verified | Email is the login |
| Session | hash of token, user, device name, created, last used | Idle timeout 21 days (Figma's default) |
| Access token | hash, user, name, scopes, expiry | For the CLI, MCP and scripts; shown once |
| Team | id, name, owner | Every user gets a personal team on sign-up |
| Membership | team, user, role: owner / admin / editor / viewer | Roles pass down to folders and files |
| Folder | id, team, name, colour, created, trashed | Figma calls them projects; the UI says folders |
| File | id, team, folder (none: a draft), owner, name, kind (design/board), created, edited, thumbnail, trashed | Content stays in `files/<id>/snapshot.bin + log.bin` |
| Grant | file or folder, user, role | Adds access on top of the team role, never removes it |
| Star | user, file or folder | Private to each person; the sidebar's Starred list |
| Recent | user, file, opened at | The last 200 per person |
| Version | file, snapshot, created, by, name, description | Autosaved after 30 minutes of edits; named on request |

**Drafts** are files with no folder, private to their owner and anyone they invite, as in Figma.
Moving a draft into a folder needs edit access to the file and the folder.

**Trash.** Deleting sets `trashed`; the file disappears for everyone and shows in the Trash of
whoever can edit it, with **Restore** and **Delete forever**. Restoring puts it back in its folder
(into Drafts if that folder is gone). A trashed folder takes its files with it. The server purges
trash after `trash_days` (default 30, configurable); Figma publishes no number.

**Where it is kept.** The metadata goes in the data directory as small JSON files, written
atomically like the server's existing `meta.json` and `links.json`, and held in memory. That
suits a team server with thousands of files and needs no database. If it outgrows that, the same
tables move to SQLite.

## Saving storage

- **Compressed snapshots:** snapshots are written zstd-compressed. Design documents are highly
  repetitive, so this should save several times their size; it is measured when built.
- **Images stored once:** image bytes go to `blobs/<sha256>`, and documents refer to them by hash
  (as Figma's `imageRef` does). The same picture in ten files or a hundred versions is stored once.
  Blobs nothing refers to are swept daily.
- **Versions share images and are pruned:** a version is a compressed snapshot without its images.
  Unnamed autosaves older than `history_days` (default 30, like Figma's free plan) are pruned; named
  versions are kept until someone deletes them.
- **Thumbnails** are small WebP images rendered by the app when it saves, at most 640 px wide.
- **Limits:** per-team storage quotas (`max_team_mb`), so one team can't fill the disk.

## Signing in

- **Email and password.** Passwords are hashed with argon2id. Sign-up sends a verification email
  when the server has SMTP settings; without them it skips verification and the operator approves
  accounts with `figcraft-server users approve`. Forgot-password emails a one-hour reset link.
- **The desktop app signs in through the browser**, as Figma's does. **Sign in** opens the
  server's sign-in page in the system browser; after sign-in the browser hands a one-time code back
  to the app on a `127.0.0.1` loopback port, and the app swaps it for a session token, with PKCE so
  an intercepted code is useless. The server serves its own sign-in, sign-up and reset pages as
  plain HTML forms (no JavaScript), on the port it already uses.
- **The browser build** signs in on the same pages and keeps the session in an HTTP-only cookie.
- **Several accounts** can be signed in at once and switched from the account menu (Figma allows
  10), each on its own server.
- **Tokens on this computer** are kept in the system keychain where there is one (macOS Keychain,
  Windows Credential Manager, Secret Service on Linux), otherwise in FigCraft's settings folder,
  readable by your user only. They are never logged.
- **Later:** Google sign-in (OpenID Connect; needs a Google Cloud client id), two-factor
  authentication with an authenticator app and recovery codes, and email invites.
- **Left out:** SAML, SCIM, SMS codes, seats and billing.

Existing share links and join codes keep working, and so does anonymous publishing on a public
server. An account adds to them; it doesn't replace them.

## Permissions

| Role | Can |
|---|---|
| Viewer | Open and comment |
| Editor | Create, rename, move, trash and restore files and folders; edit files |
| Admin | All of an editor's, plus invite and remove people and change roles |
| Owner | All of an admin's, plus rename or delete the team and transfer ownership |

The server checks every request against the role: listing, opening (joining a live session),
editing (a viewer's batches are refused, as a view link's are today), and every metadata change.

## The Home screen (the client)

The sidebar matches the UI3 screens: the account menu (avatar, name, email, Change theme,
Settings, Add account, Log out), Search, **Recents**, the team switcher with **Drafts**, **All
folders** and **Trash**, then **Starred** folders. On the right: **Recently viewed / Shared files /
Shared folders** tabs, file cards (thumbnail, kind badge, name, "Edited 5 minutes ago", owner
avatar), folder cards (three thumbnails, folder icon, "Updated 1 hour ago"), a folder page with
**Create** (Design file, Board, Folder, Import), Trash with **Files / Folders** tabs and **Date
trashed**, sorting and a grid/list switch. Signed out, Home shows your local files and a **Sign in**
button. Every action is a command (`account.signIn`, `team.switch`, `folder.create`,
`file.moveTo`, `file.trash`, `file.restore`, `file.deleteForever`, `file.star`, …), so menus,
shortcuts, the CLI and MCP do the same.

## Plan and estimate

| Phase | What | Estimate |
|---|---|---|
| 1 | Server: users, sessions, sign-in pages, browser sign-in from the desktop app, account menu | 14–22 h |
| 2 | Server: teams, roles, folders, drafts, trash, stars, recents, thumbnails; role checks on joins and edits | 16–26 h |
| 3 | Home file browser matching the UI3 screens, with the kit's icons | 14–22 h |
| 4 | Colour picker matching the UI3 screen | 3–5 h |
| 5 | Storage savings: zstd snapshots, image blobs by hash, version history with pruning | 10–16 h |
| 6 | Deploy to the VPS: TLS reverse proxy, systemd, backups | 2–4 h |
| Later | Google sign-in, two-factor authentication, email invites, access tokens | 10–16 h |

Each phase lands in its own commits with tests, and `cargo xtask ci` passes before each one.
Phases 3 and 4 can start before phase 2 is finished, against local files.

## Sources (public)

- How Figma's multiplayer technology works — figma.com/blog/how-figmas-multiplayer-technology-works
- Making multiplayer more reliable (checkpoints and journal) — figma.com/blog/making-multiplayer-more-reliable
- How Figma scaled to multiple databases — figma.com/blog/how-figma-scaled-to-multiple-databases
- Version history — help.figma.com/hc/en-us/articles/360038006754
- Team permissions — help.figma.com/hc/en-us/articles/360039970673
- Updates to how drafts work — help.figma.com/hc/en-us/articles/18409526530967
- Delete and restore files — help.figma.com/hc/en-us/articles/360047512294
- Guide to the file browser — help.figma.com/hc/en-us/articles/14381406380183
- Create a Figma account — help.figma.com/hc/en-us/articles/360039811114
- Log in or add accounts — help.figma.com/hc/en-us/articles/360041064554
- Idle session timeout — help.figma.com/hc/en-us/articles/14376092335127
- OAuth apps and PKCE — developers.figma.com/docs/rest-api/oauth-apps
- Image hashes — developers.figma.com/docs/plugins/api/Image
