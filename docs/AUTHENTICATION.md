# Accounts, authentication, and authorization

The initial identity system uses local usernames/passwords and public registration,
with PostgreSQL as the control-plane database. Authentication establishes a user;
source/media authorization is checked separately. Uploads remain in scope: ordinary
users will eventually contribute through an upload API or their own source nodes.
Permission to upload is distinct from permission to register a directory on the API
host. This change does not implement uploads or change acquisition default-deny.

## Local setup

Install and start PostgreSQL (18 is used for integration verification). With a local
Ubuntu PostgreSQL installation, open its administrative console:

```sh
sudo -u postgres psql
```

Create a dedicated application role and database. `\password` prompts without putting
the password in shell or SQL command history:

```sql
CREATE ROLE grimoire LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
\password grimoire
CREATE DATABASE grimoire OWNER grimoire;
\q
```

From the repository, configure the backend:

```sh
cd backend
cp .env.example .env
chmod 600 .env
```

Edit `.env` with that role's credentials. URL-encode special characters in the
password portion of `DATABASE_URL`. Keep `.env` out of Git. For local HTTP testing,
the example explicitly enables loopback development mode. Open the frontend using
exactly `http://localhost:5173` to match `GRIMOIRE_APP_ORIGIN`.

Create the administrator before starting the server:

```sh
cargo run -- bootstrap-admin YOUR_USERNAME
```

The command prompts twice for a password. It never accepts passwords in command
arguments. Database migrations run automatically. Bootstrap creates a new user,
refuses if any administrator already exists, and never promotes an existing public
account. Server startup refuses until an active administrator exists. Public
registration always creates a regular account.

If you used the previous single-user version, explicitly import its directory list:

```sh
cargo run -- import-legacy-sources YOUR_USERNAME
```

This imports `data/sources.json` (or `GRIMOIRE_DATA_DIR/sources.json`) transactionally
as private sources owned by the selected active administrator. It preserves source
IDs and the original JSON file, prevents duplicate imports, and aborts on conflicts.
No files are moved or deleted. Skip this command for a fresh installation.

Start the backend with `cargo run`, and run `npm install` and `npm run dev` in `web/`.
Use **Create an account** for public registration. New accounts have no sources or
media access. The initial administrator can see imported sources and connect server
directories. Owners can grant per-user visibility and media access using the
[source permissions editor](SOURCE_PERMISSIONS.md). There is no implicit administrator
bypass for another owner's media. User uploads remain a separate planned workflow.

## Account operations

Sign out in the sidebar. **Account settings** allows changing your password after
verifying the current one; this revokes all existing sessions and issues a fresh
session for the current browser. Passwords are 15–128 Unicode characters (at most
1,024 UTF-8 bytes), allow spaces/pasting, and have no arbitrary composition rules.
Usernames are case-insensitive ASCII letters/digits/underscore/hyphen, 3–32 characters.

A server operator can recover or suspend an account locally:

```sh
cargo run -- reset-password USERNAME
cargo run -- disable-user USERNAME
cargo run -- enable-user USERNAME
```

Password reset prompts without echoing and revokes all user sessions. Disable and
enable also revoke sessions. These are privileged **local operator** commands, not
public HTTP endpoints. Do not give untrusted users access to the backend environment,
database credentials, or host filesystem. No web administrator password-reset UI,
email recovery, or email verification is included.

## Implemented controls and boundaries

- Passwords: salted Argon2id hashes (RustCrypto Argon2 defaults: 19 MiB, two passes,
  one lane). CPU work runs off async workers with four concurrent password tasks;
  saturation fails with 429 instead of building an unbounded queue.
- Sessions: independently generated 256-bit opaque IDs in HttpOnly, SameSite=Strict,
  host-only cookies with Path=/. Only SHA-256 session-token hashes are stored in
  PostgreSQL. Tokens rotate on login/password change; logout deletes the record.
  Absolute lifetime is 12 hours with a 30-minute idle timeout, enforced server-side.
  Each account retains at most ten sessions. Expired rows are cleaned on login.
- HTTPS cookies use the `__Host-` prefix and Secure by default. Insecure cookies
  require explicit loopback HTTP development mode and use a separate cookie name.
  Sessions survive API restarts through PostgreSQL. Disabled users are checked on
  every protected request; role changes take effect on the next request.
  Session issuance rechecks the verified password hash and active-account state
  under a user-row lock, serializing with password resets/changes, operator account
  updates and concurrent logins. A password verification against a replaced hash
  cannot issue a later session; a currently disabled account cannot get one. The same lock enforces the ten-session cap under
  concurrent logins. Successful login auditing commits with session issuance;
  rejected rechecks use the generic login error and failed-login audit event.
- CSRF: all state-changing API requests require the exact configured Origin.
  Authenticated mutations additionally require the session's synchronizer token in
  `X-CSRF-Token`. The token is held in frontend memory and restored from `/auth/me`;
  session cookies are never accessible to frontend JavaScript or localStorage.
  Login and registration require JSON and the configured origin as well.
- Input/query handling: account request bodies are capped at 4 KiB, unknown signup
  fields are rejected, database queries are parameterized, and login failures use
  the same response for unknown/disabled accounts and wrong passwords. Unknown
  usernames incur a dummy Argon2 verification to reduce timing differences.
  Signup reports username availability because usernames are account identifiers.
- Default-deny authorization: unauthenticated sources/scans/streams/books return
  401. Source queries require ownership or explicit grants; unauthorized source IDs return 404. Only
  administrators may register API-host directories. Registration supplies no roles,
  source grants, upload grants, or acquisition grants. Media and byte-range requests
  recheck the session and ownership/explicit media grants every time. Removing a source denies future requests.
- Files already delivered to a browser, decoded book pages, and a streaming response
  already in progress cannot be recalled on logout or revocation. New requests are
  denied. Local filesystem writers remain trusted; the existing path/symlink checks
  do not eliminate filesystem replacement races with a malicious local process.
- Throttling: PostgreSQL-backed atomic counters currently allow 30 login requests
  per connecting IP / 15 minutes, ten per normalized username / 15 minutes, five
  registrations per connecting IP / hour, and ten password changes per user / 15
  minutes. Attempts include successful requests. These are initial operational
  settings, not acquisition quotas. Counters are shared across API instances.
  Forwarded-IP headers are deliberately ignored: behind Vite or a reverse proxy,
  its connecting IP shares the limit. A trusted-proxy model and edge limits are
  required before expanding public traffic; do not simply trust X-Forwarded-For.
- Audit records track registrations, login outcomes, logout, password changes,
  bootstrap/import, and operator account changes without passwords or raw session
  tokens. Auth responses and protected responses use `private, no-store`.

## Deployment requirements and remaining work

The API still binds to loopback (port 3000 by default; `GRIMOIRE_PORT` can override
the port without changing the loopback binding). Serve the web app and `/api` behind the same HTTPS
origin, set `GRIMOIRE_APP_ORIGIN=https://your-host`, remove the development HTTP flag,
and keep PostgreSQL private. Remote PostgreSQL connections require
`sslmode=verify-full` and a trusted database CA; local loopback/Unix-socket connections
are an explicit exception. Back up PostgreSQL, protect migration/database privileges,
and establish retention for auth events. Local JSON is now only an import format;
PostgreSQL stores users, sessions, source registrations, ownership/grants, and
per-user saved items/open history. Personal media state never grants source access.

This is an authentication foundation, not a claim that public-launch security is
complete. Followups include MFA/passkeys, breached-password screening, self-service
recovery, bot/account-abuse protections, trusted proxy handling, security monitoring,
and operational backup/recovery. Uploads need their own app-managed storage roots,
quotas, file validation, processing isolation, and media ownership/sharing policies.
They must not accept arbitrary host filesystem paths. Contributor-hosted nodes and
public signup do not imply public media access.

The implementation follows [OWASP Authentication](https://cheatsheetseries.owasp.org/cheatsheets/Authentication_Cheat_Sheet.html),
[Password Storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html),
and [Session Management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
guidance for the controls described above.

## Verification

```sh
cd backend
cargo test
# Separately, against a disposable PostgreSQL test server/role with CREATE DATABASE:
DATABASE_URL='postgresql://TEST_ROLE:TEST_PASSWORD@localhost/TEST_DATABASE' cargo test -- --ignored
```

The ignored suite also includes an MKV/ASS extraction test requiring FFmpeg,
FFprobe and DejaVu Sans (`fonts-dejavu-core` on Ubuntu); see [SUBTITLES.md](SUBTITLES.md).
The database tests use sqlx-managed temporary databases. Use a dedicated test role
and server, not production credentials. They exercise public registration, role
injection, forged/missing cookies, CSRF/origin checks, source ownership, default-deny
media access, session rotation/logout/expiry/disable, password changes, and throttling.
`npm test` and `npm run build` in `web/` cover the frontend and reader regressions.

## Sign out other sessions

Account settings now provides **Sign out other sessions** independently of changing
password. Enter the current password and use that button; new-password fields may
remain empty. The current session/cookie/CSRF token remains valid, while all other
sessions for that account are deleted. Other users are unaffected. Already delivered
media or in-flight responses remain subject to the existing revocation limits.

`POST /api/auth/logout-others` accepts only `{current_password}` and requires the
normal authenticated Origin/CSRF checks. Password verification uses bounded Argon2
workers, with ten attempts per account per fifteen minutes. The operation rechecks
that the account password has not changed and that the calling session remains
active, and records `other_sessions_revoked` without credentials. It does not change
the account password or prevent a subsequent valid login. Disposable-database tests
cover wrong passwords, CSRF rejection, revocation, and current/other-user isolation.

## Active-session activity

Account settings displays up to ten active sessions, with the current session
first, sign-in time and last activity. Refresh takes a new snapshot; the list is
also refreshed after password changes or signing out other sessions. The existing
password-confirmed sign-out policy is unchanged. Session expiry remains thirty
minutes idle / twelve hours absolute.

`GET /api/auth/sessions` requires authentication and selects only the caller's
sessions, including for administrators. It filters expired/idle sessions and
returns only `created_at`, `last_seen` (epoch seconds) and `is_current`. No raw
session tokens, hashes, CSRF values, or other accounts' session data are returned.
The response is private/no-store. This feature adds no device or IP tracking.
