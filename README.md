# BranchBox
### *Parallel development for humans and AI agents — real environments, zero collisions.*

[![Release](https://img.shields.io/github/v/release/branchbox/branchbox)](https://github.com/branchbox/branchbox/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/branchbox/branchbox/total)](https://github.com/branchbox/branchbox/releases)
[![CI](https://github.com/branchbox/branchbox/workflows/CI/badge.svg)](https://github.com/branchbox/branchbox/actions)
[![License](https://img.shields.io/github/license/branchbox/branchbox)](LICENSE)

**[Website](https://branchbox.dev)** · **[Documentation](https://branchbox.dev/docs)** · **[GitHub](https://github.com/branchbox/branchbox)**

BranchBox is an open-source engine for **isolated, fully provisioned development environments** designed for engineers working with AI coding agents, or anyone juggling multiple features at once.

Every feature becomes its own **self-contained workspace** with:

- A dedicated Git worktree  
- Its own devcontainer  
- Its own Docker network  
- Its own database  
- Its own ports  
- Its own environment variables  
- Optional Cloudflare or SSH tunnels  
- Shared tool credentials mounted safely  

You get **real dev environments**, not lightweight sandboxes.  
Your agents get safe rooms to operate.  
Your main workspace stays clean.

If you’ve ever run multiple features or agents in parallel and felt things colliding, leaking, or breaking — BranchBox solves that. It makes parallel development a first-class workflow.

---

## Why BranchBox exists

Modern engineering is shifting toward **agentic, parallel development**:

- Humans work on one feature  
- AI agents explore another  
- LLMs run migrations, refactors, codegen, and experiments  
- Several ideas progress at once  

The problem: none of our tools were built for this.

- Git branches collide  
- Docker networks & ports collide  
- Devcontainers drift  
- Databases clash  
- Shared credentials leak across contexts  
- And it’s too easy for one environment to break another

BranchBox adds the missing layer:

> **A safe, predictable compute system where every feature has its own fully isolated, fully functional dev environment.**

Start one feature or ten.  
Work alone or with multiple agents.  
Nothing touches anything else unless you want it to.

---

## What BranchBox gives you

### 1. True isolation  
Every feature gets its own:

- Compose project  
- Docker network  
- Ports  
- `.env` file  
- Database  
- Devcontainer  

Zero collisions. Zero shared state unless explicitly mounted.

### 2. Real development environments  
Not an agent-only sandbox — a **full stack** environment:

- Rails, Node, Python, or generic  
- Containers, Docker Compose, databases  
- Shared credentials mounts  
- VS Code + Cursor devcontainers  
- Built-in adapter system for detecting stacks  

If it runs locally, BranchBox can isolate it cleanly.

### 3. Parallel workflows that feel effortless  
Start five features at once.  
Run five containers.  
Open five devcontainers.  
Agents run jobs while you code in another branch.

The mental overhead stays low, and the environments stay clean.

### 4. Agent-ready by design  
An always-on BranchBox Agent tracks:

- Feature events  
- Heartbeats  
- Stack metadata  
- Control-plane connectivity  
- macOS app integration over gRPC  

Agents can safely:

- Start new worktrees  
- Run minimal-mode spikes  
- Apply prompt seeds  
- Tear environments down  
- Reuse shared credentials  

Everything stays observable and isolated.

### 5. A workflow built from real-world usage  
BranchBox exists because of daily engineering pain:

- Running multiple projects in parallel  
- Hitting laptop limits  
- Letting agents work independently  
- Needing guaranteed isolation  
- Avoiding devcontainer drift  
- Avoiding "I broke main" moments  

It’s built to support how modern development actually works — especially when humans and LLM agents collaborate.


---

## Installation

BranchBox is available via **Homebrew** (recommended) or direct installation.

### **macOS (Homebrew)**
```bash
brew install branchbox/tap/branchbox
```

### **Linux/macOS (installer script)**
```bash
curl -fsSL https://raw.githubusercontent.com/branchbox/branchbox/main/install.sh | bash
```

Then open a new terminal or run:
```bash
hash -r
```

---

## Quick Start

```bash
# Initialize once (creates registry, checks environment)
branchbox init

# Start a fully isolated feature workspace
branchbox feature start "Add OAuth Integration"

# Work inside the new isolated environment
cd ../oauth-integration/
```

Your feature now has:

- Its own DB  
- Its own Docker network  
- Its own ports  
- Its own devcontainer  
- Its own environment variables  

Prefer a disposable sample project?

```bash
./scripts/setup-sample-workspaces.sh
branchbox init
branchbox feature start "Demo Feature"
```
