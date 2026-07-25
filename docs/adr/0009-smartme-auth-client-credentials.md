# ADR 0009 — smart-me authentication: OAuth2 Client Credentials (primary) + Basic (fallback)

- **Status:** Accepted
- **Date:** 2026-07-24
- **Related:** Story 1.6 (smart-me-client auth), Story 1.1 (audit spike), issues #18, #8, #1
- **Supersedes:** the original architecture assumption of `Authorization: ApiKey <key>` as the primary smart-me auth mechanism.

## Context

smart-me exposes several authentication mechanisms (per <https://api.smart-me.com/swagger>):

**OAuth applications**
- **Confidential** — server-side apps that can keep a secret; recommended flows: authorization-code + PKCE, or device-code (both **interactive** — require a human to log in and consent).
- **Public** — web/mobile apps that cannot keep a secret; same interactive flows.
- **Device (Client Credentials)** — *"Can be used as a replacement for basic authentication in embedded devices. Allows login and access to your smart-me data to anyone who knows the secret of this app. Only supports the OAuth flow 'client credentials'."*

**OAuth refresh tokens** — long-lived tokens tied to an interactive consent.

**Basic Auth** — account username/password (enabled on this account: user `gcorbaz`).

The author's account has one **Device (Client Credentials)** app configured — name `gateway`,
its Client ID and secret are held only in the local, gitignored `.env` (never in the repo).

`smartme_mqtt` is a **headless 24/7 daemon**: there is no interactive user session at runtime,
so the authorization-code/PKCE/device-code/implicit flows (which require a human to log in and
consent) are inappropriate. The only non-interactive machine-to-machine flow smart-me offers is
**client credentials** — which is exactly what the "Device" app type provides, and which smart-me
explicitly positions as *the replacement for Basic auth in embedded devices*.

## Decision

The `smart-me-client` crate authenticates as follows in v1:

1. **Primary — OAuth2 Client Credentials grant.** Exchange `SMARTME_CLIENT_ID` +
   `SMARTME_CLIENT_SECRET` for a bearer access token, then send `Authorization: Bearer <token>`
   on every API call. The client caches the token and refreshes it on expiry (and re-authenticates
   on a `401` after a valid prior token).
2. **Fallback — HTTP Basic.** `SMARTME_BASIC_USER` / `SMARTME_BASIC_PASSWORD` (account
   credentials), used only if client-credentials is not configured.
3. **TLS mandatory** for all traffic; hard-fail if TLS is unavailable (NFR13) — especially for the
   Basic fallback.

The original `Authorization: ApiKey <key>` mechanism is **not** used for this account and is
superseded by client credentials.

## Consequences

- `smart-me-client` gains a token-acquisition + expiry/refresh path (a small state: current token +
  expiry instant), driven by the injected `Clock` (no hardcoded `now()`).
- Configuration variables (see `.env.example`): `SMARTME_CLIENT_ID`, `SMARTME_CLIENT_SECRET`
  (primary); `SMARTME_BASIC_USER`, `SMARTME_BASIC_PASSWORD` (fallback). Secrets live only in
  `.env` (perms `0600`, gitignored, never logged — NFR12).
- **Resolved by Story 1.1 (2026-07-25):** the token endpoint is
  **`POST https://api.smart-me.com/oauth/token`** (discovered via
  `/.well-known/openid-configuration`; `client_credentials` grant listed, auth methods
  `client_secret_post`/`client_secret_basic`). A real exchange with the `gateway` app
  returned `{access_token, token_type: Bearer, expires_in: 3600}` and the token was
  accepted on `GET /Devices` + `GET /Devices/{id}` (HTTP 200, scope `device.read`).
  See ADR 0004 for the captured payload/`Date`-header audit.
- Error classification (Story 1.6 / Epic 2): a failed token exchange or `401`/`403` is a **fatal**
  auth error (stop + surface as `you`-culprit), distinct from transient `429`/`5xx`/timeout.
- If the client secret is lost or leaked, the smart-me OAuth app must be deleted and recreated
  (smart-me does not allow secret rotation in place) — a note for the troubleshooting guide.

## Notes

- The interactive OAuth flows remain available for a future scenario (e.g. a multi-user or
  consumer-facing variant), but are explicitly out of scope for this personal headless bridge.
