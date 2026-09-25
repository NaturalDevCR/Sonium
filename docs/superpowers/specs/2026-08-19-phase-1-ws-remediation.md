# Sonium Phase 1 WebSocket Remediation

## Goal

Close the remaining Phase 1 WebSocket authentication gap without accepting a
long-lived bearer token in a URL, WebSocket message or unauthenticated upgraded
connection.

## Design

An authenticated HTTP request obtains a random, single-use, short-lived WebSocket
ticket. Clients offer that ticket via `Sec-WebSocket-Protocol`; the server
validates and consumes it before the WebSocket upgrade, then retains the claims
only for periodic revocation checks. Tickets are bounded, expire quickly and are
invalidated by the user's session version. The HTTP API never accepts bearer
tokens in URL query strings.

The state deletion path releases its clients write lock before persistence or
external removal hooks, using a captured removal event so it cannot self-deadlock.

## Acceptance criteria

- A JWT never appears in a WebSocket URL or WebSocket message.
- Tickets expire, are one-use, session-version-bound and verified pre-upgrade.
- Replayed, expired, revoked and malformed tickets cannot upgrade a socket.
- Existing WebSocket clients stop receiving events after revocation.
- Client deletion with a persistence store completes without recursive lock
  acquisition.
