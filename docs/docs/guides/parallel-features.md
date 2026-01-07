---
sidebar_position: 1
---

# Working on Multiple Features

The most common BranchBox use case: juggling multiple features without collisions.

## The Scenario

You're working on a payment refactor. PM pings you — urgent bug in auth. You need to:
1. Save your current work
2. Switch contexts completely
3. Fix the bug
4. Switch back without losing anything

**Without BranchBox:** Git stash, branch switching, port conflicts, database state confusion.

**With BranchBox:** Two isolated workspaces, zero interference.

## The Workflow

### Start Your First Feature

```bash
branchbox feature start "Payment refactor"
cd ../payment-refactor
# Work here...
```

### Urgent Bug Comes In — Start Another Feature

Don't switch branches. Just create a new isolated workspace:

```bash
cd ../main  # Back to your main worktree
branchbox feature start "Fix auth bug"
cd ../fix-auth-bug
# Fix the bug here...
```

You now have two completely isolated environments:
- `../payment-refactor/` — Your refactor, untouched
- `../fix-auth-bug/` — The urgent fix

Each has its own:
- Git branch
- Docker containers
- Database
- Ports
- Environment variables

### Switch Between Them Freely

```bash
# Check on your refactor
cd ../payment-refactor
docker compose ps  # Your containers are still running

# Back to the bug fix
cd ../fix-auth-bug
git commit -m "Fix auth validation"
```

No stashing. No port conflicts. No "which branch am I on?"

### Finish and Clean Up

```bash
# Bug is fixed, tear it down
branchbox feature teardown fix-auth-bug

# Continue your refactor
cd ../payment-refactor
```

## Tips for Multi-Feature Work

### Check What's Running

```bash
branchbox feature list
```

Shows all active features with their status, mode, and last update time.

### Use Descriptive Names

```bash
# Good — clear intent
branchbox feature start "Add Stripe webhook handler"

# Less good — vague
branchbox feature start "webhooks"
```

The title becomes the branch name (`feature/add-stripe-webhook-handler`) and the folder name (`../add-stripe-webhook-handler/`).

### Resource Limits

Each feature runs its own containers. With 5+ features, you might hit resource limits. Options:

1. **Use minimal mode for exploration:**
   ```bash
   branchbox feature start "Quick spike" --minimal
   ```
   Skips heavyweight modules (devcontainer, compose).

2. **Tear down features you're not actively using:**
   ```bash
   branchbox feature teardown old-feature
   ```

3. **Keep branches, remove resources:**
   ```bash
   branchbox feature teardown old-feature --keep-branch
   ```
   Removes containers/worktree but keeps the git branch for later.

## Real Example: 3 Features at Once

```bash
# Morning: Start the main feature
branchbox feature start "User dashboard redesign"
cd ../user-dashboard-redesign
# Work on dashboard...

# Afternoon: Urgent security fix
cd ../main
branchbox feature start "Patch XSS vulnerability"
cd ../patch-xss-vulnerability
# Fix, test, commit, push

# Evening: Quick experiment
cd ../main
branchbox feature start "Try new caching strategy" --minimal
cd ../try-new-caching-strategy
# Experiment freely without affecting anything

# Check status
branchbox feature list
# 📚 Feature registry — 3 active
# user-dashboard-redesign   Active  full     ...
# patch-xss-vulnerability   Active  full     ...
# try-new-caching-strategy  Active  minimal  ...

# Clean up the experiment
branchbox feature teardown try-new-caching-strategy
```

---

**Next:** [Working with AI Agents](ai-agents.md) — Give your coding agents safe sandboxes
