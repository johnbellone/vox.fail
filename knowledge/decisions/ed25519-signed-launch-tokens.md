---
type: Decision
title: "Decision: Signed Tokens Instead of Server Call-backs"
description: "The directory signs a player's identity with Ed25519 so worlds verify it offline with only a public key."
tags: [decision, security, auth, ed25519]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Ed25519 Signed Launch Tokens

**Context**: many world servers must trust the directory's sign-in without calling it on every join.

**Decision**: a token `v1.<postcard claims>.<signature>` with account, name, character and expiry, valid 24 h, signed with the directory's private key. Worlds load only `directory.key.pub`.

**Consequences**: no runtime dependency from world to directory for sign-in; worlds can run anywhere; a leaked private key would let anyone mint identities (keep `directory.key` private); a stolen token works until it expires (no revocation list); name or character changes appear only on the next token.

See `architecture/identity-and-tokens`.
