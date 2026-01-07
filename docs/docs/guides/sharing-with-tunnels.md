---
sidebar_position: 4
---

# Sharing with Tunnels

Let reviewers, PMs, or stakeholders see your feature running live — before it's merged.

## The Problem

You're working on a feature. PM wants to see it. Options:

1. **Deploy to staging** — Slow, blocks other work
2. **Screen share** — Awkward, not async
3. **Record a video** — Outdated immediately
4. **"Just pull my branch"** — They need your whole dev setup

## The Solution: Cloudflare Tunnels

BranchBox can provision a Cloudflare tunnel for your feature:

```bash
branchbox feature start "New checkout flow"
```

If the tunnel module is enabled, you get a public URL:

```
+------------------+------------+------------------------------------------+
| Tunnel           | ✓ online   | cloudflared (checkout-abc123.trycloudflare.com) |
+------------------+------------+------------------------------------------+
```

Share `https://checkout-abc123.trycloudflare.com` with anyone. They see your feature live.

## How Tunnels Work

1. **BranchBox starts your feature** with its isolated environment
2. **Cloudflared creates a tunnel** from Cloudflare's edge to your local service
3. **You get a public URL** that routes to your local Docker container
4. **No firewall changes** — outbound-only connection

The URL points to your feature's isolated environment, not your main workspace.

## Enable Tunnels

Tunnels require Cloudflared installed:

```bash
# macOS
brew install cloudflare/cloudflare/cloudflared

# Linux
# See https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/install-and-setup/installation/
```

BranchBox will auto-detect Cloudflared and provision tunnels during feature start.

## Manual Tunnel Management

### Open a Tunnel for an Existing Feature

```bash
branchbox tunnel open my-feature
```

Output:
```
🌐 Tunnel
  Feature: my-feature
  Status: online
  Hostname: my-feature-xyz789.trycloudflare.com
```

### Remove a Tunnel

```bash
branchbox tunnel remove my-feature
```

The feature keeps running, but the public URL is gone.

### Check Tunnel Status

```bash
branchbox feature list
```

Shows tunnel status for each feature:
```
Feature       Status  Tunnel
------------  ------  ---------------------------------
my-feature    Active  online (my-feature-xyz789.trycloudflare.com)
other-feature Active  —
```

## Tunnel Statuses

| Status | Meaning |
|--------|---------|
| **online** | Tunnel is active, URL works |
| **pending** | Tunnel is being provisioned |
| **manual** | Cloudflared not installed, manual setup needed |
| **disabled** | Tunnel module skipped |

## Security Considerations

**Quick tunnels** (trycloudflare.com) are:
- ✅ Great for demos and reviews
- ✅ No account required
- ✅ Auto-generated URLs
- ⚠️ Publicly accessible (anyone with the URL)
- ⚠️ Not for production traffic

For sensitive features, consider:
- Adding authentication to your app
- Using a Cloudflare Zero Trust tunnel (requires account)
- Sharing URLs only with trusted reviewers

## Example: PM Review Workflow

```bash
# Start feature with tunnel
branchbox feature start "Redesign user profile"
cd ../redesign-user-profile

# Work on it...
docker compose up -d

# Check the tunnel URL
branchbox feature list

# Share with PM
# "Hey, check out the new profile page: https://redesign-abc123.trycloudflare.com/users/profile"

# PM gives feedback, you iterate
# They can refresh anytime to see updates

# Done with review, remove tunnel but keep working
branchbox tunnel remove redesign-user-profile
```

## Skip Tunnels When Not Needed

If you don't need external access:

```bash
branchbox feature start "Internal refactor" --skip-module tunnel
```

No tunnel provisioned, faster startup.

---

**Next:** [CLI Reference](../reference/cli.md) — Every command and flag
