# Decisions — Ants

Why Ants is shaped the way it is: the part of TinyBrains' decision record about this
repository. One line per decision, with the reasoning kept and the cost of flipping it named where
that was worked out.

> **The record was one file until 17 September 2026**, `devops/docs/decisions.md`. When devops
> stopped running anything (N25) it was split, so each decision lives in the repository it is
> about. **The numbers are the record's, not this file's**: they were assigned once across the
> platform and are never reused, so a citation of `41` or `N24` names one decision wherever it now
> lives, and a section number below is the one the whole record gave it.
>
> **Four numbering series.** The **A-series** is the twenty-one architectural decisions taken
> before anything was built. The **plain series** is the build decisions the layers took, numbering
> from 1 again, so `A5` and `5` are different decisions and a bare number in a code comment means
> the plain series. The **R-series** is the Orion 1.8.1 rebuild and the **N-series** the runner, the
> submission path and where each repository's artifacts come from.

## Where the rest of the record is

| Decisions | Where they live |
|---|---|
| **A1–A21**, §2's review findings and the Orion changes asked for | [soma](https://github.com/Tiny-Brains/soma/blob/main/docs/decisions.md) |
| Plain series: the match table (2, 3, 7, 7c, 7d, 18, 21, 22), the clocks (1, 7–13, 23, 24, 28, 51–59), admission (20, 35–40), the retired loader (6, 34, 46, the adapter cap) and deployment 43, 44, 48 | [soma](https://github.com/Tiny-Brains/soma/blob/main/docs/decisions.md) |
| Plain series: the wave (19, 33) and deployment 5, 25, 41, 42, 45 | [kalam](https://github.com/Tiny-Brains/kalam/blob/main/docs/decisions.md) |
| Plain series: the game and the protocol (4, 14–16), the baselines (49, 50 of the loader's) | [ants](https://github.com/Tiny-Brains/ants/blob/main/DECISIONS.md) |
| Plain series: the training environment (47, 48 of the loader's) | [cli](https://github.com/Tiny-Brains/cli/blob/main/DECISIONS.md) |
| Plain series: deployment 47 and 49 (the compose file's) | [web](https://github.com/Tiny-Brains/web/blob/main/DECISIONS.md) |
| **R1, R2, R4, R6, R9, R10, R11** · **R3, R7, R8** · **R5** | soma · kalam · ants |
| **N3, N6–N8, N12, N13, N15–N19** · **N1, N2, N4, N5, N9** · **N20–N22, N24** · **N23** · **N25** | soma · kalam · ants · cli · web |
| Still open | the repository each is forced in: 30, 31, N14 and three unnumbered in soma; N10 and a runner on another network in kalam; 32 in ants; 26, 27, N11 and the orchestrator in web |

The plain series collides with itself once: the retired loader's **47, 48, 49** and deployment's
**47, 48, 49** are different decisions, told apart above by where each lives.

---

## 3. Build decisions

Numbered as the build numbered them. Each names the repository it now lives in.

### The loader — ~~`axon`~~, **superseded 14 September 2026**

> **Every decision in this section was taken about a service that no longer exists**, and the
> checkout is gone from the working tree. Orion 1.8.1's `models` entity replaced it whole — §4's
> R-series is what replaced each one. They are kept because a
> decision log that deletes what it superseded cannot be read backwards: **46** (there is no compute
> cap) and **49** (the baselines are a repository of their own) still stand on their own arguments,
> and the rest are the reasoning the R-series answers. Where an entry below and the R-series
> disagree, the R-series is what runs.

| # | Decision | Taken as |
|---|---|---|
| **49** | *(amended by N20 — the repository is `ants/baselines/` now; the rest stands)* **The baselines are trained artifacts in a repository of their own** | `Tiny-Brains/ants-baselines`, a competitor repository the platform owns, with no special access. One fixed teacher distilled into each weight class (the size/fidelity curve the classes exist to measure) plus a method column at micro (one class, one dataset, several learners). It submits through the ordinary gate, which also means the ordinary gate gets exercised by something other than a fixture |
| **50** | **Above `mini`, the byte cap stops being the binding constraint** | Measured, not predicted. A row owns `turn_ms / rows` of a play call — 31.2 ms at `wave_k` 16 — and a fully convolutional network over the largest board runs out of turn at about 170,000 parameters, which is inside Mini. `small` reaches its share at roughly a quarter of its 4 MiB cap; no dense architecture reaches `large`'s at all. The classes are not wrong, but the top two are byte budgets nobody can currently spend, and closing that means parameters that are *read* rather than multiplied. Tracked in `ants-baselines` (`README.md`, `classes.toml`) |

### The game and the protocol — [`ants`](https://github.com/Tiny-Brains/ants)

| # | Decision | Taken as |
|---|---|---|
| 4 | The adapter instruction budget | **1,000,000** — see decision 6 above; this is the same number reached from the protocol side |
| 14 | Players per match | **the preset decides.** The number of players is a property of the *map*, so a preset names a world and how many play it. A version's rating mixes seat counts, which is fine: map coverage spreads every version across the same preset distribution, so any king-making from a larger map inflates sigma rather than biasing mu |
| 15 | `wave_state` encoding | **an opaque base64 blob over a packed binary encoding** — smaller, and it makes "nothing outside the cartridge parses game state" a property rather than a promise |
| 16 | Forfeit and resignation | **a forfeited seat plays no-ops and is ranked last by Kalam**; no resign value in the action schema yet |
| — | Ordering of `mine` | **deterministic, not stable** — row-major by position, so a determinism audit reproduces without the ordering becoming an identity channel. Going from "meaningless" to "stable" later breaks no adapter; the reverse breaks every one |
| — | What `water` carries | **`water AND seen`**, a per-player seen-mask. The only reading that matches what the field is documented to mean — a model is a pure function of one observation and has no channel to accumulate a map itself. It costs: the masks are 60% of `wave_state`, and the observation grows monotonically over a match |

---

## 4. The 1.8.1 rebuild — the R-series

Taken 14 September 2026, when Orion 1.8.1 made its `models` entity a strict superset of what `axon`
does. Each is **measured where it could be measured**, and the numbers are in the rows themselves —
taken with `orion-server dry-run --model-dir` against the real baselines, not estimated. A third
numbering series, because these overturn A-series decisions rather than extending the build series.
The study they came out of is in this repo's git history (`docs/migratingv18.md`, deleted
15 September 2026); what survived it is here and in [`orion-notes.md`](https://github.com/Tiny-Brains/soma/blob/main/docs/orion-notes.md) §0, with the
three measurements still owed named in this repo's `README.md` Status block.

| # | Question | Decision | What it overturns, and what it cost to check |
|---|---|---|---|
| R5 | Who computes the visibility plane? | **The cartridge.** `observe` sends `vis` — the disk union it already computes twice a turn — and an adapter reads it with the `rle_expand` it already uses for `water`. `tb.dilate` is withdrawn, not ported | The protocol's *"derivable, so not sent"*. True in numpy, false in JSONLogic: **verified** that an inner iterator cannot address the enclosing one's element (`{"val":[[1],…]}` reads the innermost, every higher level reads the root), so the per-ant disk is a 241-fold unrolled kernel or nothing. A rule of the game now has one implementation |

---

## 4b. The N-series — a runner leaves the deployment, and GitHub leaves the submission path

Taken and built 16 September 2026, as two tracks decided together because each removed a dependency
that was not earning its place. They shared one thread — the models bucket, which lets a runner read
artifacts without a secret and is the submission path's audit trail — and no file. The proposal and
its phased plan (`docs/design.md`, `docs/design-plan.md`) were deleted once they were all record;
what they argued is here and in [`architecture.md`](https://github.com/Tiny-Brains/soma/blob/main/docs/architecture.md) §3a, the statements and routes
are `soma/docs/schema.md` §3.8a, §4 and §4a, the operator's page is [`deployment.md`](https://github.com/Tiny-Brains/kalam/blob/main/docs/deployment.md)
§11, and what each phase turned up is in the Status blocks of `soma`, `kalam`, `devops` and `web`.
N10, N11 and N14 are still open, in §5.

### The baselines' repository

| # | Question | Decision | What it overturns, and what it cost |
|---|---|---|---|
| N20 | Are the baselines a repository of their own? | **No. `ants-baselines` moves into `ants` as `baselines/`**, with its own Python toolchain and guide, excluded from the cartridge's image by `.dockerignore` and run by nothing in `build.sh`. Its `games.toml` resolves the cartridge at `..`; `scripts/dev/seed-baselines.sh` defaults to `../ants/baselines`; ants-starter installs `tb_baselines` from `git+https://github.com/Tiny-Brains/ants#subdirectory=baselines`. The competitor-facing repository is ants-starter, and it stays one | The "repository of its own" half of **49**. What a baseline encodes is what the cartridge sends, so an observation change was a commit in each repository — R5's `vis` was one in `ants` and one in `ants-baselines` the same day — and the conformance test that proves the two agree ran against a sibling checkout nobody pinned. One repository makes that one commit. The "no special access" half stands: the entries are still seeded and admitted exactly as before, and the component still knows nothing about them. **Proved not to reach the engine:** the ants image built after the move is the image built before it, component `sha256:185a2845…` both times. **Cost:** `baselines/` and the `tb_baselines` package name are now a public path of `ants`, because every starter clone pip-installs from it; and training commits share a log with engine-digest changes |

### Where a competitor's cartridge comes from

| # | Question | Decision | What it overturns, and what it cost |
|---|---|---|---|
| N21 | Does a competitor need `ants` and `devops` checked out beside their own repository? | **No. The cartridge is a pinned GitHub release**: `ants/tools/release.sh --publish` packs the image's `/artifacts/` into one deterministic `ants-artifacts.tar.gz` on a release tagged `engine-<12 hex>`, and a registry pins `repo`, `release`, the archive's sha256 and the `engine` digest. The CLI downloads it once into `~/.cache/tinybrains/cartridges/<archive digest>/`, refuses it unless the archive and the component inside it hash to those two, and reads the unpacked tree exactly as it reads a `dist/`. drill and ants-starter pin it; ants-starter's baseline seat is a URL at a pinned `ants` commit. The platform's own consumers keep `path` and images | The registry's `release` mode, which fetched `component` and `manifest` as two files and so could play a match and nothing else — `check` has no reference observations, `view` no viewer, `maps export` no boards — which is why no registry ever used it and every competitor-facing README said "clone ants, clone devops, build the image with Docker". An image was the other candidate and is what the platform consumes, but it needs Docker, which drill promises it does not. Committing `/artifacts/` into drill was the third, and is a copy per competitor repository that nothing compares. **Cost:** a new engine is a release cut and a new block in two `games.toml` files, and **no check compares their `engine` with what `ANTS_REF` ships**, so a drill can pin an engine the ladder no longer plays; `conform` refuses a ladder replay by name when it does. A competitor still needs a Rust toolchain for `cargo install --git` until the CLI ships binaries |

### One starter kit per game

| # | Question | Decision | What it overturns, and what it cost |
|---|---|---|---|
| N22 | Is practice a repository of its own? | **No. `drill` is retired and archived, and each game has one competitor repository, `<game>-starter`**, holding only what a newcomer needs to begin: the trained entry and `train.py`, a `games.toml` pinning the game's release (N21), two match files — the entry against itself and against a baseline fetched by URL at a pinned commit — and a CI check. What drill carried beyond that moved to where it is read once rather than copied per game: **the match-file format is §Match files of the book's *Testing* chapter**, the boards travel in the release (`tinybrains maps export`), and what its untrained fixtures taught — the two head shapes, fixed axes failing other board sizes, a colony that never moves — was already in *What your model answers*, *Model format* and *Testing*'s idle-colony replay. Its every-board, wave and baseline-vs-baseline files are one row edit from the documented format and were not carried | drill, which existed because the starter did not yet: a competitor cloned two repositories that played the same matches, and drill's match files seated an untrained fixture rather than a model worth watching. Folding all of drill in was the first plan and doubled the starter's files; a starter a newcomer can read in one sitting was preferred. **Cost:** the fixtures and the committed board export are gone from every clone (the archived repository keeps them); `drill/models/nano-bc` and `micro-bc` were the unchecked copies of `ants/baselines`, and the starter's URLs replace them, so a match file with a baseline seat needs the network on every run. `scripts/dev/submission-storm.py` now clones `ants/baselines/models/micro-bc` by default |

### The cartridge's release

| # | Question | Decision | What it overturns, and what it cost |
|---|---|---|---|
| N24 | Does the cartridge ship as an image? | **No. `ants` has no Dockerfile and publishes GitHub releases only**, cut by its own `build` workflow: every push runs the gate on an `ubuntu-24.04-arm` runner, builds every artifact, packs `dist/` with `tools/pack.py`, checks the baselines' encoding, plays ants-starter against the build and reports which release it reproduces; `gh workflow run build.yml -f publish=true` publishes one tagged `engine-<12 hex>` (`-2`, `-3` for another archive of the same engine). **kalam's, web's and web/docs' Dockerfiles fetch the latest release** with curl (the ADDed `releases.atom` keys the layer cache, so a new release is fetched and an old one is not), unless `ANTS_RELEASE` names a tag, and check the viewer inside was transpiled from the component beside it; `--build-context ants=<an ants dist/>` builds any of them against an unreleased engine. kalam's package now carries `reference/observations.json` beside `cartridge.json`, and the loader reads both from it, so this repository mounts no cartridge image and `configs.sh` reads the manifest out of kalam's package and compares its engine with the book's viewer. `ants`, `ANTS_DIR` and `ANTS_REF` go | Decision **N21**'s "the archive is the image's `/artifacts/`" and the 10 September rule that every artifact ships as an image. The image did two jobs, and neither needed a container: it pinned rustc and remapped build paths (now `rust-toolchain.toml` and `build.sh`, so a checkout builds the same way CI does), and it carried the tree to three Dockerfiles (now a download, as the CLI's binary already was for the book). Measured first: a plain Debian arm64 container with rustup's 1.98.1 and `build.sh`'s remaps builds `sha256:df312c04…`, the image's digest, byte for byte, and `tools/pack.py` re-packs the release's archive to `sha256:6a2a11aa…`. **Cost:** the digest depends on rustc's host — an Apple-silicon Mac builds the same source, flags and paths into other bytes — so the runner's architecture is part of the engine and a laptop's digest is never the ladder's; **publishing is deploying**, because the next build of kalam, web or the book takes a new release with no commit anywhere, under a live season too, unless `ANTS_RELEASE` is pinned; three images built either side of a release carry two engines, which `configs.sh` catches between kalam and the book but not web's own viewer; and a build now needs GitHub reachable where it used to need only a local image |

---

## 5. Still open

| # | Decision | Forced at | Note |
|---|---|---|---|
| 32 | Was dropping `turn` from Ants right | public launch | cheap, and endgame behaviour might turn on it; a model can infer lateness from ant counts and map control. Worth an A/B once there is a ladder |
