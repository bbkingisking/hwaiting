FSRS-backed vocab builder for Korean. Uses a “blanked sentences” card paradigm; more specifically, a single-word cloze deletion embedded in a full sentence with L1 gloss structure. Live demo with 50 cards on https://hwaiting-demo.surge.sh/. 

## Quick start

Clone, build backend, build frontend, launch:

```sh
git clone "https://github.com/bbkingisking/hwaiting"
cargo build --release --manifest-path hwaiting/backend/Cargo.toml
(cd hwaiting/frontend && deno install --frozen-lockfile && deno task build)
HWAITING_ADMIN_PASSWORD=password HWAITING_JWT_SECRET=$(openssl rand -base64 32) HWAITING_STATIC_DIR=./hwaiting/frontend/dist ./hwaiting/backend/target/release/hwaiting
```

Assuming defaults (see Configuration below) don't clash with anything else on your system, you should get a fully running instance with 50 cards on http://127.0.0.1:3000 that you can login with username `admin` and the password you passed to the `HWAITING_ADMIN_PASSWORD` variable. 

## Configuration

Every backend option is configurable from an env var `KEY_NAME` or a plain text file in `$CREDENTIALS_DIRECTORY/key-name`. For example, both `HWAITING_ADMIN_USERNAME=admin` and a file in `$CREDENTIALS_DIRECTORY/hwaiting-admin-username` containing the string `admin` accomplish the same thing. Env vars take precedence over plain text files.

| Variable | Credential file | Required | Default |
| --- | --- | --- | --- |
| `HWAITING_DATABASE_URL` | `hwaiting-database-url` | Only if `XDG_DATA_HOME`/`HOME` can't be resolved | `sqlite://$XDG_DATA_HOME/hwaiting/hwaiting.db` (or `~/.local/share/hwaiting/hwaiting.db`) |
| `HWAITING_ADMIN_USERNAME` | `hwaiting-admin-username` | No | `admin` |
| `HWAITING_ADMIN_PASSWORD` | `hwaiting-admin-password` | Yes | - |
| `HWAITING_JWT_SECRET` | `hwaiting-jwt-secret` | Yes | - |
| `HWAITING_RP_ID` | `hwaiting-rp-id` | Only if `HWAITING_RP_ORIGINS` is set | unset (with `HWAITING_RP_ORIGINS`) = passkey sign-in disabled |
| `HWAITING_RP_ORIGINS` | `hwaiting-rp-origins` | Only if `HWAITING_RP_ID` is set | unset (with `HWAITING_RP_ID`) = passkey sign-in disabled |
| `HWAITING_HOST` | `hwaiting-host` | Only if not using systemd socket activation or `HWAITING_UNIX_SOCKET` | `127.0.0.1` |
| `HWAITING_PORT` | `hwaiting-port` | Only if not using systemd socket activation or `HWAITING_UNIX_SOCKET` | `3000` |
| `HWAITING_UNIX_SOCKET` | `hwaiting-unix-socket` | No | unset (falls back to `HWAITING_HOST`/`HWAITING_PORT`) |
| `HWAITING_COOKIE_SECURE` | `hwaiting-cookie-secure` | No | `false` (supports both HTTP and HTTPS); set `true` for HTTPS-only deployments |
| `HWAITING_COOKIE_PARTITIONED` | `hwaiting-cookie-partitioned` | No | `false`; `true` requires Secure and enables CHIPS for cross-site deployments |
| `HWAITING_COOKIE_SAME_SITE` | `hwaiting-cookie-same-site` | No | `lax`; also accepts `strict` or `none` (`none` requires Secure) |
| `HWAITING_JWT_EXPIRY_SECONDS` | `hwaiting-jwt-expiry-seconds` | No | unset or `0` = tokens never expire |
| `HWAITING_STATIC_DIR` | `hwaiting-static-dir` | No | unset = API-only mode, no static file serving |
| `HWAITING_CORS_ALLOWED_ORIGINS` | `hwaiting-cors-allowed-origins` | No | unset = no CORS layer (same-origin only) |

## Slow start

This section goes into more detail about the stack, the database shape, the different auth options, and deployment tips.  

### Stack

This repo contains a Rust backend and a static React frontend. However, the backend is designed to be completely agnostic as to who consumes it. The backend can serve a static website itself (via the `HWAITING_STATIC_DIR`/`hwaiting-static-dir` configuration, see above) or operate as a headless API (if `HWAITING_STATIC_DIR`/`hwaiting-static-dir` is not set) that anyone can call.

The backend can generate a complete OpenAPI contract if you run it with the `--print-openapi` CLI option (prints to `stdout` and exits immediately). That lets you bootstrap a new client/frontend much more easily. The GitHub CI workflow in this repo also publishes the latest OpenAPI contract on https://hwaitingapi.surge.sh/.

### Data shape

All data is stored in a single SQLite database. The shape of the data is highly domain-specific for Korean, and unlikely to be useful for any other language unless explicitly repurposed. A fresh backend run bootstraps the database with 50 cards, so you can inspect the standard/expected structure for the cards, how they appear in the frontend, and use that information to add your own cards. 

### Review scheduling

The pinned Rust `fsrs` 5.2.0 crate implements FSRS-6. Stability is the time in days
until predicted recall reaches **90%**, not a general-purpose review interval.
For positive decay `d = w[20]`, the model's raw interval is:

```text
factor   = 0.9^(-1/d) - 1
interval = stability / factor * (desired_retention^(-1/d) - 1)
due_at   = last_review + interval days
```

The backend calls the crate's `next_states` when grading and `next_interval` when
rescheduling; it does not duplicate this formula in SQL. The crate also upgrades
legacy 17/19-parameter models, which use a different decay from the FSRS-6 default.
See the [FSRS algorithm specification](https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm)
and the [pinned crate implementation](https://github.com/open-spaced-repetition/fsrs-rs/blob/aca2838bfbdc6f15ca3f7a0c96a99fae466c9e9c/src/inference.rs).

Intervals remain fractional days, converted to UTC deadlines rounded up to a
second. There are no additional learning steps, whole-day rounding, fuzz, or
maximum-interval cap. The existing grading/optimization convention measures elapsed
time in completed 24-hour periods, as the Rust API takes integer elapsed days;
the configurable day boundary controls daily limits/statistics, not these intervals.

Each reviewed card stores its current deadline in `cards_states.due_at` (Unix
seconds). Queue selection, due counts, and review countdowns all use it. Changing
retention, optimizing/resetting parameters, or importing data recalculates current
deadlines from stored stability and last-review time, atomically with the change.
This changes neither the memory state nor review history and does not count as a
review. `review_history.scheduled_days` preserves the interval calculated at that
historical review; it is not the source of the current deadline after rescheduling.

On upgrading from the old stability-based scheduler, startup backfills deadlines
using each user's current retention and model. No review history is replayed or
rewritten. At 90% the schedule is effectively unchanged, apart from second-level
rounding; at other retentions, existing cards may become due sooner or later.
Backups retain their existing format: deadlines are reconstructed after restoring
memory state, settings, and parameters. Invalid model parameters or unrepresentable
deadlines are rejected instead of storing unusable schedules.

### Auth

Username/password (Argon2 hashes stored in the DB) and passkey sign-in both issue the same JWT in a host-only `hwaiting-auth` cookie (`HttpOnly`, `Path=/`, `SameSite=Lax` by default). The JSON response contains profile information only. Requests authenticate through that cookie; bearer headers are no longer accepted. The JWT signing secret and `HWAITING_JWT_EXPIRY_SECONDS` retain their existing meaning: tokens have no expiry by default. Cookie persistence follows a configured JWT lifetime, or defaults to 400 days when JWT expiry is disabled (browsers may impose their own storage limits).

The same backend source supports both deployment layouts through configuration:

| Deployment | `HWAITING_COOKIE_SECURE` | `HWAITING_COOKIE_SAME_SITE` | `HWAITING_COOKIE_PARTITIONED` |
| --- | --- | --- | --- |
| Production, with both HTTP LAN and HTTPS access | `false` | `lax` | `false` |
| Demo, HTTPS frontend and API on different sites | `true` | `none` | `true` |

The production row is the default and permits login over either protocol. `Secure=false` permits HTTPS as well as HTTP; it does not disable HTTPS. These flags are explicit because the backend may sit behind a TLS-terminating proxy.

For the cross-site demo, also list the frontend's exact origin in `HWAITING_CORS_ALLOWED_ORIGINS`. The cookie is issued and cleared with `HttpOnly; Secure; SameSite=None; Partitioned`: CHIPS-capable browsers keep the API cookie in a separate partition for the frontend's top-level site, allowing authentication without unpartitioned third-party cookies. Visiting the API directly or from a different top-level site uses a different partition. Older browsers without CHIPS may still block cross-site authentication. Partitioning requires HTTPS and should be enabled only for the demo deployment, not the production HTTP/HTTPS deployment.

Every state-changing API request (`POST`, `PUT`, `PATCH`, `DELETE`), including login, must carry `X-Requested-With: hwaiting`. This is a CSRF defense: HTML forms cannot set the header, and cross-origin JavaScript must pass a preflight against the explicit CORS allowlist. Both clients set it automatically. Read-only requests need no custom header, so native `EventSource` can authenticate with cookies.

`POST /api/auth/logout` clears the cookie, including an expired or invalid one. As before, JWTs are stateless: clearing a client cookie does not revoke another copy of that JWT. Existing browser users and agent clients must sign in again after upgrading; browser local-storage tokens are removed and the agent removes its legacy token file after a successful login.

In addition to that, the backend supports passkeys for identifier-less sign-up/sign-in flows. The passkey protocol requires an https:// context (or localhost). In order for it to work, the `HWAITING_RP_ID` and `HWAITING_RP_ORIGINS` should be set accordingly. With passkey auth, the only WebAuthn-specific data stored per passkey is the public key: `passkeys.public_key`, populated at sign-up time. At sign-in, *every* public key in the database is checked for signature verification, and the first matched one gets logged in. There is deliberately no credential_id or a user identifier blob stored for quicker matching, as I was experimenting with how minimal the webauthn protocol can be while still remaining functional. The drawbacks of storing a public key *only* lie strictly outside the cryptography: unable to verify signature provenance via device metadata (backup-able), unable to check sign-in counters, etc. 

### Deployment

The most simple deployment is by running the binary and binding to an IP:PORT. You can see an example of that in the Quick start section above. If needed, the runtime can be restricted heavily without compromising functionality. Below is an example of an extremely restricted deployment using systemd.

As a standard web server, the backend speaks TCP/IP by default and uses that to receive and send data via HTTP requests. However, `axum`, the backend crate powering the server, has fairly good support for UNIX sockets as well. By using sockets instead of network requests, the backend can be completely network-free (and offload the network communication to a more battle-tested surface, such as a reverse proxy). This is fully optional and configurable via `HWAITING_UNIX_SOCKET`. If a reverse proxy is already in the picture (common when hosting to clients other than localhost), then this is more or less free sandboxing.

In addition to that, by targetting `x86_64-unknown-linux-musl`, it can be compiled into a fully static binary. That makes it easy to `chroot` it in its own restricted filesystem without worrying about dynamic linking breaking. Combined with the unix socket approach, this means you can run this binary in a completely networkless/filesystemless environment. 

Here is an example systemd unit file that provisions a very restricted runtime: 

```sh
[Unit]
Description=Hwaiting Backend Service
After=network.target hwaiting.socket
Requires=hwaiting.socket

[Service]
DynamicUser=yes
StateDirectory=hwaiting
StateDirectoryMode=0700
RootDirectory=/opt/hwaiting/root
MountAPIVFS=yes

Environment=HWAITING_DATABASE_URL=sqlite:/var/lib/hwaiting/hwaiting.sqlite3
Environment=HWAITING_STATIC_DIR=/srv/hwaiting

LoadCredentialEncrypted=hwaiting-jwt-secret:/etc/credstore/hwaiting/hwaiting-jwt-secret.cred
LoadCredentialEncrypted=hwaiting-admin-password:/etc/credstore/hwaiting/hwaiting-admin-password.cred
ExecStart=/usr/local/bin/hwaiting

NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
PrivateNetwork=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
ProcSubset=pid
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
RemoveIPC=yes
KeyringMode=private
UMask=0077
CapabilityBoundingSet=
AmbientCapabilities=
SupplementaryGroups=
RestrictAddressFamilies=none
SystemCallFilter=@system-service
SystemCallErrorNumber=EPERM
SystemCallArchitectures=native
IPAddressDeny=any
MemoryDenyWriteExecute=yes
PrivateUsers=yes
SystemCallFilter=~@resources
SystemCallFilter=~@privileged

[Install]
WantedBy=multi-user.target
```

A few things to make this work: 

- A .socket service like: 

```sh
[Unit]
Description=Hwaiting Backend socket

[Socket]
ListenStream=/run/hwaiting.sock
SocketMode=0660
SocketUser=root
SocketGroup=caddy

[Install]
WantedBy=sockets.target
```

- The backend binary dropped in `/opt/hwaiting/root/usr/local/bin/hwaiting`
- The frontend in `/opt/hwaiting/root/srv/hwaiting`
- Encrypted credentials in `/etc/credstore/hwaiting/hwaiting-jwt-secret.cred` and `/etc/credstore/hwaiting/hwaiting-admin-password.cred`. You can generate the .cred files with `systemd-ask-password | sudo systemd-creds encrypt --with-key=tpm2 --name=hwaiting-admin-password - hwaiting-admin-password.cred` and `systemd-ask-password | sudo systemd-creds encrypt --with-key=tpm2 --name=hwaiting-jwt-secret - hwaiting-jwt-secret.cred`.
- A reverse proxy that forwards the web address to the socket. An example Caddy block: 

```
:8080 {
    reverse_proxy unix//run/hwaiting.sock
}
```

Using this setup, the binary: 

- Does not have access to any network interfaces.
- Does not have access to the filesystem outside `/opt/hwaiting/root`.
- Does not see any of the users on the machine.
- Cannot create its own sockets. 
- Cannot make any syscalls that are in the `@resources` and `@privileged` filters. 

And other, more minor protections.
