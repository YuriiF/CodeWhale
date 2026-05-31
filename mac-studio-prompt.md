# Mac Studio CodeWhale Handoff

Use this when moving the external SSD to the Mac Studio and resuming the
v0.8.48 release-readiness work.

## Source Of Truth

- Checkout: `/Volumes/VIXinSSD/whalebro/codewhale`
- Branch: `harvest/v0.8.48-community`
- Lightweight Desktop migration bundle:
  `/Volumes/VIXinSSD/whalebro/migration-local-only/desktop-codewhale-light-20260530-180216`
- Bundle setup notes:
  `/Volumes/VIXinSSD/whalebro/migration-local-only/desktop-codewhale-light-20260530-180216/MAC_STUDIO_SETUP.md`

The migration bundle intentionally excludes duplicate repos, `target/`,
`node_modules/`, `.git/objects`, `.next/`, `dist/`, and build caches. It keeps
small local notes/configs and git metadata only. Review any copied env/config
files before placing them under `~/.codewhale`.

## First Commands On The Mac Studio

```bash
cd /Volumes/VIXinSSD/whalebro/codewhale
git status --short --branch
git log --oneline --decorate -5

cargo build --release -p codewhale-cli -p codewhale-tui

mkdir -p ~/.npm-global/bin
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codewhale ~/.npm-global/bin/codewhale
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codewhale-tui ~/.npm-global/bin/codewhale-tui
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/codew ~/.npm-global/bin/codew
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/deepseek ~/.npm-global/bin/deepseek
ln -sfn /Volumes/VIXinSSD/whalebro/codewhale/target/release/deepseek-tui ~/.npm-global/bin/deepseek-tui

codewhale --version
codewhale-tui --version
```

## Smoke Checks

```bash
codewhale doctor

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
  | codewhale mcp-server \
  | head -n 1
```

## Sanitized Previous Handoff Context

- The last stale-binary symptom was traced to an old local launcher, not a
  current runtime bug. After moving machines, rebuild first and relink launchers
  before judging runtime behavior.
- Xiaomi/MiMo now supports a `providers.xiaomi.cluster` setting for `cn`, `sgp`,
  and `ams`. An explicit provider `base_url` or `MIMO_BASE_URL` still wins over
  the cluster setting.
- MiMo Token Plan keys are cluster-specific. Do not assume a key that works in
  one region will work in another; configure the cluster/base URL to match the
  issued key.
- The generic provider resolver names were cleaned up from old
  `deepseek_*` wording to `active_provider_*`; real DeepSeek identifiers,
  environment variables, and legacy path fallback names intentionally remain.
- Issue #2363 was fixed by correcting `/provider` wording in five locales.
- Wanjie Ark's default documented model moved to `deepseek-v4-pro`; live catalog
  verification still needs a Wanjie key.
- Website facts were regenerated for v0.8.48 and should show Xiaomi MiMo in the
  provider list.
- US-facing web deployment should start with Railway using `web/railway.json`.
  Keep Cloudflare as the edge/cron/KV route until the curator storage path is
  replaced.
- 0.9.0 whale-pods work should stay next-cycle scope, not a late v0.8.48
  release add-on.

The original private handoff file contained machine-local setup details and
credential-adjacent notes. Keep it out of git history.

## All-PR Harvest Goal

Primary Mac Studio objective: get as many open community PRs merged as possible
before publishing v0.8.48, without lowering the release bar. Treat "merged if
possible" as:

1. Directly merge PRs that are small, coherent, CI-clean, and release-aligned.
2. Harvest the useful parts of PRs that are valuable but dirty, conflicted,
   too broad, missing tests, or mixed with scratch files.
3. Defer only when the PR is clearly v0.9.0 scope, unsafe, unreviewable, or
   blocked on secrets/live provider access.

Start by refreshing state rather than relying on this file:

```bash
cd /Volumes/VIXinSSD/whalebro/codewhale
git switch harvest/v0.8.48-community
git pull --ff-only

/opt/homebrew/bin/gh pr list --repo Hmbown/CodeWhale --state open --limit 200 \
  --json number,title,author,headRefName,baseRefName,isDraft,mergeable,updatedAt,labels,url \
  > .private/open-prs-latest.json

/opt/homebrew/bin/gh issue list --repo Hmbown/CodeWhale --state open --limit 200 \
  --json number,title,author,updatedAt,labels,url \
  > .private/open-issues-latest.json

/opt/homebrew/bin/gh pr checks 2382 --repo Hmbown/CodeWhale
```

Known release-harvest PR:

- `#2382` is the draft PR for `harvest/v0.8.48-community` into `main`. It was
  mergeable at last check. After all checks are green and any additional PR
  harvests are folded in, mark it ready and merge it.

Known community queue seed from the laptop session, to refresh on Mac Studio:

- Already folded or partially folded: `#2364`, `#2375`, `#2373`, `#2366`,
  `#2357`, `#2344`, `#2330`, `#2324`, `#2302`, `#2283`, `#2275`, `#2273`.
- High-value next candidates to inspect carefully: `#2377`, `#2371`, `#2367`,
  `#2358`, `#2356`, `#2355`, `#2354`, `#2347`, `#2343`, `#2336`, `#2333`,
  `#2331`, `#2326`, `#2325`, `#2320`, `#2319`, `#2316`, `#2314`, `#2313`,
  `#2311`, `#2305`, `#2304`, `#2301`, `#2298`, `#2297`, `#2296`, `#2295`,
  `#2291`, `#2290`, `#2289`, `#2287`, `#2285`, `#2281`, `#2280`, `#2279`,
  `#2278`, `#2277`, `#2276`, `#2274`.
- Draft/stacked/broad PRs should be harvested only when the patch is clean and
  releasable; otherwise keep them for v0.9.0.

Suggested loop for every PR:

```bash
N=2377
/opt/homebrew/bin/gh pr view "$N" --repo Hmbown/CodeWhale \
  --json number,title,author,body,headRefName,baseRefName,isDraft,mergeable,statusCheckRollup,files,commits,url
/opt/homebrew/bin/gh pr diff "$N" --repo Hmbown/CodeWhale --patch --color never > ".private/pr-$N.patch"
```

Then classify:

- `merge-now`: coherent change, no release risk, tests/docs present or trivial
  to add. Merge/cherry-pick into `harvest/v0.8.48-community`, preserve author
  credit, run focused tests, update changelog/readmes/release credits.
- `harvest`: useful idea but patch is mixed, stale, or broad. Re-implement the
  smallest correct version locally, cite the PR/issue in changelog credits, and
  leave a PR comment explaining what was harvested.
- `defer`: v0.9.0 architecture, new product surface, provider live-key required,
  security-sensitive, or not safe to ship late. Label/comment with the reason.
- `close-after-harvest`: when the releasable fix has landed elsewhere, comment
  with the commit/PR that shipped it and close only if maintainer policy allows.

Use merge commits for clean community branches when practical so author history
survives:

```bash
git fetch origin "pull/$N/head:pr/$N"
git merge --no-ff "pr/$N" -m "harvest: merge PR #$N into v0.8.48"
```

If the PR is conflicted or contains unrelated files, do not force it. Apply only
the correct files/hunks with `git apply --3way` or re-implement manually, then
commit with a message that names the PR and contributor.

For every accepted PR or harvested contribution:

- Add or update tests proportional to risk.
- Update `CHANGELOG.md` and remember that `crates/tui/CHANGELOG.md` is a
  symlink to it.
- Update `README.md`, `README.zh-CN.md`, and `README.ja-JP.md` when the change
  affects public behavior or contributor credits.
- Update website copy in both `web/app/[locale]/...` languages when the web
  claims or install/deploy docs change.
- Update `.github/workflows/release.yml` release body contributors. The release
  body must include direct credits, not just a changelog link.
- Run `./scripts/release/check-versions.sh` after credit changes.

Minimum validation before pushing a new harvest batch:

```bash
cargo fmt --all -- --check
git diff --check
./scripts/release/check-versions.sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

cd web
npm run lint
npm run build
cd ..
```

For very large batches, run focused tests first, but do not mark v0.8.48 ready
until the full gates above and PR #2382 CI pass.

## Release Readiness Prompt

Paste this into a fresh CodeWhale session after the rebuild:

```text
Use parallel sub-agents. Agent A: inspect this checkout for v0.8.48 release/docs drift around `.codewhale` vs `.deepseek` paths and provider lists. Agent B: inspect release artifact/npm wrapper paths for missing assets, especially Windows `codewhale.bat` and updater hints. Agent C: inspect runtime liveness risks around sub-agent fanout, compaction/status UI, and MCP tool discovery. Do not edit files. Return a release-risk table with exact file references, confidence, and one recommended follow-up test.
```

## Current Release Notes

- v0.8.48 GitHub Release was not live at last check.
- The release workflow body includes direct contributor credits; do not publish
  a release body that only links to `CHANGELOG.md`.
- If the release already exists when resuming, verify:

```bash
/opt/homebrew/bin/gh release view v0.8.48 --repo Hmbown/CodeWhale --json body
```

The body must include a `## Contributors` or `## Credits` section with the
material contributors for v0.8.48.
