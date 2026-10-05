---
name: branchbox-release-videos
description: Produce BranchBox launch films and focused documentation videos from verified features, app captures, and release evidence; use for BranchBox launch or release media, not ordinary application testing.
---

# BranchBox release videos

Use a dedicated video producer subagent alongside implementation and documentation agents. The producer owns the beat map, evidence manifest, motion source, render, and media QA; the documentation agent embeds the approved outputs. Keep this work independent of release tagging or publication authorization.

Read [references/production.md](references/production.md) for the supplied continuous-take design language and technical rules. The maintained example at `assets/film.html` is an executable, deterministic `seek(t)` source; `scripts/render.mjs` produces stills, movies, and QA inputs.

## Start from evidence

Infer BranchBox as the known brand. Gather fresh, high resolution app captures and a feature/claim list from the current release. A screenshot of a plan proves the plan's UI, not its execution. Distinguish setup from launch, registry Active from a running container, configured integrations from live outcomes, and branch retention from preserving ignored files. Do not invent cloud provisioning, public tunnel URLs, notifications, or successful cleanup. Use disposable repositories without private source or credentials for captures.

Ask only for missing inputs that materially affect production: music/SFX with an explicit license, or optional footage. Do not require the supplied script's print-business photos or wall clip when they do not fit BranchBox. Build a silent edition while audio is absent; label it silent and never synthesize the script's downloaded SFX.

Show the beat map and four proof stills (open, glass, stage, final workspace) before writing the full film. Use the current manifest at `assets/manifest.json` as a concrete example, replacing stale captures and receipt pointers for each release.

## Produce a small set

Default to a 27-second square launch film (54 beats at 120 BPM) and two 12–15-second focused videos tied to the changed release features. A live walkthrough is a separate artifact and must retain its actual capture provenance. Use meaningful continuous masks and shape transformations, not crossfades or a sequence of static title cards. Keep text readable at website embed sizes.

Quick defaults skip devcontainer, compose and specs. Tunnel workflow provisioning is controlled separately by request/configuration and policy; other enabled or enforced modules may still run. For this release the focused examples explain Quick versus Full setup and reviewing teardown/ownership. The teardown film must state that Git-ignored files are removed with the worktree even when keeping the branch. Do not animate a captured confirmation as if it executed.

## Deliver and integrate

Render at 1440×1440, 60 fps, with four deterministic subframes per output frame. Check one frame per beat, every source screenshot, start/end loop, full decode, duration, and single-frame difference spikes. Record exact asset hashes, source revision, whether audio exists, claim provenance, renderer settings, and unresolved limitations beside the source.

Place only reviewed MP4s and posters in `docs/static/media/` and `docs/static/img/mac-app/`. Coordinate distinct filenames with the documentation agent; include accessible text equivalents or transcripts and native video controls. Preserve existing genuine walkthroughs. Build the combined docs/website and check responsive embeds after integration.

During releases, refresh videos only when affected features or UI changed. Keep unchanged valid media rather than re-recording every release. Before announcing, compare media claims with the shipped version, then use the already authorized launch workflow. This skill does not authorize tags, publication, purchases, or messages.
