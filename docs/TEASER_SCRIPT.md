# BranchBox 60s Teaser — Producer Script

Audience: Solo engineers and agent power users. Goal: spark excitement to try BranchBox.
Duration: 45–60 seconds (main). Optional 15s social cut at end.

Core message: Stop context switching. Run multiple features in parallel—safely. One command to spin up isolated worktrees; one command to sync devcontainers across all features.

Prep
- Use the demo runner: `scripts/demo-teaser.sh --stack rust` (fast, container-friendly).
- Optional: record the terminal with a dark theme. Hide distractions. Increase font size (16–18px).
- If recording VS Code/Cursor, ensure the “Reopen in Container” prompt shows cleanly.

Structure (beats)
1) Hook (0–5s)
   - On-screen: “Stop context switching. Run multiple features—simultaneously.”
   - Terminal with prompt ready.

2) Create (5–20s)
   - Narration: “Initialize your project and spin up an isolated feature.”
   - Terminal:
     - `branchbox init`
     - `branchbox feature start "Add OAuth Integration" --skip-module compose --skip-module database`
   - Show the BranchBox checklist: worktree path, branch, adapter, modules (partial ok), color.

3) Open in Container (optional B-roll, 3–5s)
   - Quick shot: VS Code/Cursor detects `.devcontainer/` → “Reopen in Container”.
   - Narration: “Open in your devcontainer—everything’s scoped to this feature.”

4) Parallelism (20–35s)
   - Narration: “Start a second feature—no collisions.”
   - Terminal:
     - `branchbox feature new backlog-quick-fix --minimal --default-prompt`
     - `branchbox feature list`
   - On-screen: two features listed, distinct colors/branches; “DB/ports/network isolated”.

5) Devcontainer Sync (35–50s)
   - Narration: “Edit `.devcontainer/` once; replay to every feature.”
   - Terminal:
     - Edit a file: `echo "// tweak" >> .devcontainer/devcontainer.json`
     - `branchbox devcontainer sync --dry-run` (or without `--dry-run` for a stronger punch)

6) CTA (50–60s)
   - On-screen: “BranchBox — Try it in 2 minutes”
   - URL: `github.com/branchbox/branchbox`

Script (voiceover)
- “Stop context switching. With BranchBox, you run multiple features in parallel—safely.”
- “Initialize your project and spin up an isolated worktree: your own branch, devcontainer, and environment variables.”
- “Start a second feature in minimal mode for quick spikes or agent experiments.”
- “Devcontainer changed? Replay updates across all features with one command.”
- “BranchBox — Try it in 2 minutes.”

Lower-thirds/Overlays (suggested)
- “Isolated worktrees”
- “Per-feature devcontainers, DB, networks”
- “Minimal mode for agent ‘yolo’”
- “Sync devcontainers across all features”

Recording options
- Live: run `scripts/demo-teaser.sh --stack rust` and screen record the terminal. Use 60s pacing.
- Auto-GIF (for socials): `vhs scripts/demo-teaser.tape` (requires `ffmpeg` + `ttyd`).

Social 15s cut (optional)
- Terminal supercut only: feature start → feature list → devcontainer sync.
- Overlay: “Stop context switching.” → “Parallel features.” → “Sync everywhere.”

End slate
- “github.com/branchbox/branchbox”
- “MIT licensed” (optional)
