---
type: Decision
title: "Decision: Authority Cannot Be Sent"
description: The shared currency can only be earned and spent on feats; tradeable value lives in per-world currencies.
tags: [decision, economy, game-design]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Authority Cannot Be Sent

**Decision**: Authority (AUTH) is earned by playing and by winning raids and is spent on feats (plot, portal, currency, raid). It cannot be given to another player. A world's owner may *issue* a world currency (fixed supply) that anyone can send.

**Why (inferred from the design)**: a non-transferable, rate-capped earning curve (60 per day from blocks, 1 per 10 minutes) resists farming and selling, and keeps the cost of land and portals tied to presence. Transfers happen only in world coins, whose supply the owner fixes.

**Consequences**: a world server holding the shared secret could still mint AUTH through false reports (risk 30), so per-world limits are a next step.

See `architecture/economy-and-social`.
