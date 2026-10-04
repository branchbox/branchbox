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
cd ../<your-project>  # Back to your main worktree
branchbox feature start "Fix auth bug"
cd ../fix-auth-bug
# Fix the bug here...
```

You now have two separate feature worktrees:
- `../payment-refactor/` — Your refactor, untouched
- `../fix-auth-bug/` — The urgent fix

Each has its own Git branch and working directory. Configured setup modules can add a Compose identity, devcontainer configuration, database naming, and a customized environment file. Start each container environment and application separately; fixed host ports and external databases still depend on your project configuration.

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

### Tear Down Safely

Teardown refuses, before removing anything, when a feature still holds work: uncommitted files in the worktree, or commits on its branch that aren't merged. The refusal lists the files and the branch and names the flag that would override it. Nothing is lost by trying.

```bash
# See what a teardown would do; changes nothing
branchbox feature teardown old-feature --dry-run

# Drop leftover scratch files, keep the branch
branchbox feature teardown old-feature --discard-changes --keep-branch

# The branch was abandoned: delete it with its unmerged commits
branchbox feature teardown old-feature --discard-changes --force-delete-branch
```

`--discard-changes` only discards uncommitted files; it never force-deletes the branch. `--force` keeps its older, broader meaning (remove whatever the state, and `git branch -D` when deleting the branch), so prefer the specific flags.

Cleaning up many features at once? `branchbox prune --dry-run` lists what would be lost first. Prune itself discards uncommitted changes and force-deletes branches it deletes, so for anything you are unsure about, tear features down one at a time instead. The Mac app's Prune does exactly that: it runs one safe teardown per feature and skips any that refuse.

## Real Example: 3 Features at Once

```bash
# Morning: Start the main feature
branchbox feature start "User dashboard redesign"
cd ../user-dashboard-redesign
# Work on dashboard...

# Afternoon: Urgent security fix
cd ../<your-project>  # Back to main worktree
branchbox feature start "Patch XSS vulnerability"
cd ../patch-xss-vulnerability
# Fix, test, commit, push

# Evening: Quick experiment
cd ../<your-project>  # Back to main worktree
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
