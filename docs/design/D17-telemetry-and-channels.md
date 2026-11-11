# D17 — Telemetry policy and release channels

**Status:** Part 2 (release channels) is designed and built. Part 1 (telemetry) is a
**recommendation that is pending the maintainer's decision**. The maintainer owns the
telemetry policy; until they confirm it, nothing in this document changes what the program sends,
and the program sends nothing beyond the opt-in update check.

**Satisfies:** spec 06 §6.4 (telemetry) and §6.5 (release channels, update cadence).
**Touches:** `fluut-core::update`, `fluut-core::settings`, `fluut-app` (Settings window),
`fluut-cli` (`update-check`), `xtask manifest`, `.github/workflows/release.yml` and
`channels.yml`. Builds on [D14](D14-build-test-release.md) §5.
Plan: [2026-11-11-release-channels](../plan/2026-11-11-release-channels.md).

## 1. Decisions at a glance

| # | Question | Recommendation | Who decides |
|---|---|---|---|
| T1 | Telemetry / usage analytics | **None in v1.** No analytics, no usage statistics, no crash upload. Any future telemetry needs a design change that meets the gate in §2.4 | **Maintainer (pending)** |
| T2 | Crash reports | Stay local in v1 (as [PRIVACY.md](../../PRIVACY.md) already says). Sending them is telemetry by this document and goes through the same gate | **Maintainer (pending)** |
| C1 | Channels | Two: `stable` (default) and `beta`. No nightly | Built; maintainer may veto |
| C2 | Manifest layout | One signed manifest **per channel**, same format `fluut.releases/1` | Built |
| C3 | Where the beta list lives | Rolling pre-release `update-channels`, refreshed after a person publishes a release | Built |
| C4 | Channel choice in the app | Setting `update.channel` (`stable` or `beta`), default `stable`; `fluut update-check --channel` | Built |
| C5 | Cadence | Suggestions only, §3.7 | **Maintainer** (a release-process choice) |
| C6 | Flathub | Stable on the `stable` branch; a `beta` branch of the Flathub repository for pre-releases, when Flathub is done | Follow-up |

## 2. Telemetry (pending the maintainer's decision)

### 2.1 What the project has promised

- `PRIVACY.md` opens with "This program will not transfer any information to other networked
  systems unless specifically requested by the user or the person installing or operating it", and says
  "no telemetry, no analytics". The only network use is the update check, which is off until
  the user says yes.
- The SignPath Foundation terms require that a program which sends data to unspecified networked
  systems describes it, shows it at installation and lets the user turn it off
  ([`docs/CODE_SIGNING.md`](../CODE_SIGNING.md), D14 §5.2). The simplest way to keep meeting them is to send
  nothing that the user did not ask for.
- Spec 06 §6.4 says that telemetry, if any is added, MUST be opt-in, documented and viewable, and
  that crash reports are opt-in with a viewer.
- The Linux Flatpak has no network permission at all (D14 §5.2), so telemetry could never run
  there without widening its permissions.
- D14 §6 built only the local half of crash reporting. No sending code or endpoint exists.

### 2.2 Options

| Option | For | Against |
|---|---|---|
| A. Never | Strongest promise, nothing to maintain | Cannot be kept if a future need is real (crash upload, feature use); it would need a policy change anyway |
| **B. None in v1; the door stays shut until a design change** | Keeps every present promise. A later opt-in feature is possible but expensive on purpose | Nothing learned about use except what users tell us |
| C. Opt-in anonymous usage counts in v1 | Some data on which features are used | A server to run and secure, a privacy statement to write, a viewer to build, a second network host, no Flatpak support, extra SignPath scrutiny. Most users who matter would say No, so the data would be biased |

### 2.3 Recommendation (pending the maintainer): option B

**v1 sends no telemetry and no crash reports.** In concrete terms:

1. `PRIVACY.md` stays as it is on this point. No code that sends usage data, identifiers,
   machine details or crash reports is added.
2. The only permitted network use is what `PRIVACY.md` lists: the update check (opt-in, once a day
   at most, `github.com`) and "Check now". D17 adds one URL on the same host (§3.3); it adds no new host.
3. Crash reports stay in the data folder. A user who wants to report a crash attaches the file to an issue
   (the wiki's Troubleshooting page says how).
4. What the project may learn without telemetry: GitHub's own release download counts, issues, and
   the UAT record sheets. These are not features of the program.

### 2.4 The gate for any future telemetry

A change that makes the program send anything else than the update-check requests is accepted only if
all of these hold. They are written here so a later contributor does not have to guess.

1. A **design change first**: a new design document (or a revision of this one) that names the data fields,
   the receiving host and its operator, retention, and who can read it. The maintainer approves it.
2. **Opt-in, off by default,** asked on its own, never bundled with the update-check question, never
   enabled by a default, an upgrade or a dismissed dialog (a dismissed question is a No, as for updates).
3. **A viewer:** the exact bytes that would be sent are shown in the program before the first send,
   and a log of what was last sent can be opened. A "send" button for a crash report shows the whole
   report.
4. **No project data, file names, paths, user names, IP-derived location or stable identifier.** At most a random
   per-installation value the user can reset, and only if the design shows it is needed.
5. **`PRIVACY.md` and `CODE_SIGNING.md` change in the same pull request,** with the new row in the table of
   network use, and the Windows installer shows the statement (SignPath requirement).
6. **One switch** in Settings, honoured at once, plus an environment variable that disables all
   non-update network use for managed installs.
7. **Tests:** a test that lists every URL constant of the program and fails when a host is not in the
   documented list (today: `github.com`), and a test that the default settings send nothing.
8. **Flatpak:** the network permission is added only for a feature that needs it, with a review of
   `finish-args` (the sandbox is a promise to Flatpak users).

A release must not ship telemetry code that is compiled in but dormant behind a flag the user cannot see.

### 2.5 What the maintainer must decide

- Accept option B (none in v1), or choose A or C.
- Whether crash reports may ever be uploaded (spec 06 §6.4 allows an opt-in upload; this document
  treats it as telemetry and holds it behind the gate). If yes, a later plan names the endpoint.

When the maintainer has decided, change the **Status** line above and the "pending" mark in spec 06 §6.4.

## 3. Release channels

### 3.1 The two channels

| Channel | Gets | Tag | Meant for |
|---|---|---|---|
| `stable` (default) | Releases without a pre-release suffix | `vX.Y.Z` | Everyone |
| `beta` | Stable releases **and** pre-releases | `vX.Y.Z-beta.N`, `vX.Y.Z-rc.N` | Testers who accept more faults in return for early features |

A user on `beta` is also offered a stable release once it is newer than the beta they run, so
a beta tester is never stranded on a pre-release. A user on `stable` never sees a pre-release,
even if the program they run is a pre-release (they installed it by hand). This replaces the earlier rule of
D14 §5.3 step 4 ("a pre-release only to a running pre-release"), which tied the choice to the build
instead of the person.

There is no nightly channel: a build per commit needs signing approval per build (SignPath requires a person
to approve each release), and a CI artifact is what developers already use.

### 3.2 Versions and tags

- Versions are semantic versions. Order by SemVer 2.0 precedence, as the `semver` crate does:
  `1.2.0-beta.1 < 1.2.0-beta.2 < 1.2.0-beta.10 < 1.2.0-rc.1 < 1.2.0`, and `1.2.0-beta.1 > 1.1.9`.
  Use only the identifiers `beta.N` and `rc.N` (alphabetical order puts `beta` before `rc`), with N a
  number.
- The release workflow already matches `v[0-9]+.[0-9]+.[0-9]+*`, requires the tag to equal the workspace
  version, and marks any tag with a `-` as a GitHub pre-release. So a beta release is: set the
  workspace version to `1.2.0-beta.1`, commit, tag `v1.2.0-beta.1`, push the tag. Nothing else
  changes. The stable release is a **new** commit with version `1.2.0` and a new tag: a stable build is not the beta
  binary renamed, because the version string is inside it. (Builds are reproducible given the commit, D14 §5.4.)
- The numeric part of a pre-release version is the one in the MSI (the suffix is dropped,
  `packaging/wix/build-msi.ps1`), so `1.2.0-beta.3` and `1.2.0` share the MSI version and the later one replaces the earlier.
  Going from a beta back to an older stable (`1.1.0`) is not an upgrade for Windows Installer: uninstall first. The
  update check never suggests a downgrade, so this only happens when the user does it by hand.

### 3.3 Manifest layout: one manifest per channel

GitHub's `releases/latest/download/<file>` points at the newest release that is **neither a draft nor a
pre-release**. That is why the update check works for stable today, and it means a pre-release can
never be the target of that address. GitHub has no address for "latest including pre-releases".

| Option | Verdict |
|---|---|
| **One manifest per channel** (`releases.json` for stable, `releases-beta.json` for beta), same format | **Chosen.** The stable address and file are unchanged, so no stable client has to migrate and a mistake in a pre-release cannot reach the stable list |
| One manifest for all, each release with a `"channel"` field | The one file would have to sit at one address. Only a rolling release can hold it (a beta tag cannot update `releases/latest`), so every stable user would depend on the rolling release and on every beta publication being right |
| Read GitHub's releases API (`api.github.com`) | A second host (changes `PRIVACY.md`), unsigned JSON, and a rate limit |
| GitHub Pages or another host | A new service to run and to put in the privacy statement |

How the files look (format `fluut.releases/1` is unchanged; the top-level `channel` was already there and now takes two values):

- `releases.json`: `"channel": "stable"`. Only releases without a pre-release suffix.
  `cargo xtask manifest --channel stable` refuses a pre-release version. Published with each
  stable release, read through `releases/latest/download/releases.json` as before.
- `releases-beta.json`: `"channel": "beta"`. **Every** release, pre-releases included, newest first.
  Built for every release (stable or not) from the previous beta list plus the new release.
- A release entry carries no channel of its own: the version suffix decides, in one place (SemVer), so
  the two cannot disagree.
- The signature of each is `<file>.minisig`, made with the same minisign key (§3.6).

**Where `releases-beta.json` is served.** From the rolling GitHub release `update-channels`
(a pre-release, so it is never "latest"):
`https://github.com/martinlong1978/fluutcam/releases/download/update-channels/releases-beta.json` and `.minisig`.

**How it gets there.** The release workflow builds and signs `releases-beta.json` with the other release files and
attaches it to the **draft** release, like `releases.json`. A person publishes the draft (D14 §5.2). The workflow
`.github/workflows/channels.yml` runs on `release: published` and copies the two beta files, unchanged, to
`update-channels` (`gh release upload --clobber`). It has no signing key and signs nothing; the signature the
workflow made is the one the app checks. It stops with an error when the list does not hold the published version.
The copy is made after publication so the list never points at assets of an unpublished draft.

Rules that follow: publish drafts in tag order (each beta list is the previous list plus its release); a draft is
published before the next tag is pushed (the previous list is read from `update-channels` and from
`releases/latest`). The release workflow reads the previous lists with a check on the HTTP status, so a
network fault stops the release instead of silently starting a list from nothing (404 means first
release).

### 3.4 How the app picks a channel

- Setting `update.channel` in `settings.toml`: `"stable"` or `"beta"`. Default `stable`. A missing or unknown value
  reads as `stable` (and does not discard the other settings). It is changed in **Settings → Releases**, never
  by the first-start question (the update question stays one yes/no).
- `fluut_core::update::Channel` maps a channel to its two URLs. The check requests only that channel's pair
  (list and signature), verifies the signature with the one compiled-in key, **then** requires the list's
  `channel` to equal the one asked for, and then offers the newest version greater than the running one that
  has a file for the platform. On `stable` pre-release versions are skipped whatever the running version is.
- Changing the channel clears the shown result (an answer for another channel says nothing) but does not
  start a check; the automatic check runs on its schedule (at most once a day) and **Check now** works as before.
- `fluut update-check [--channel stable|beta]` uses the saved setting unless the flag is given.
- No downgrade is ever offered. A tester who goes back to `stable` while running `1.2.0-beta.2` is told they are up
  to date until `1.2.0` ships (or until they install an older stable by hand).
- The channel is not stored anywhere but the settings file and is not sent: it shows only in which of the two
  URLs the request asks `github.com` for (§3.8).

### 3.5 Flatpak

A Flatpak updates itself and the program's update check does not run in the sandbox (no network
permission), so channels do not apply inside it. For Flathub (a follow-up of
[the Flatpak plan](../plan/2026-11-11-flatpak.md)): the app repository on Flathub builds stable from its default
branch; Flathub's convention for pre-releases is a `beta` branch in the app repository, published to the
`beta` Flatpak branch (users add the `flathub-beta` remote). Use it for `-beta.N`/`-rc.N` tags, with each `<release>` in
the metainfo marked `type="development"` for a pre-release. Check the current Flathub documentation when that
follow-up starts; none of it is needed for the GitHub release. The `.flatpak` bundle on a GitHub pre-release is a
bundle like the others and is only on that release page.

### 3.6 Signing: every channel is signed the same way

- SignPath: a pre-release is a release (it is public and installable), so the same project, signing policy and
  **person approving each request** apply. There is no separate "test" certificate. The cost to the
  maintainer is one approval per beta; this limits how often it makes sense to cut betas.
- minisign: `releases.json`, `releases-beta.json` and `SHA256SUMS` are signed with the one key whose public half is
  compiled into the app. A second key would add a second thing to protect and rotate, with no benefit: both lists
  are made by the same workflow.
- Provenance attestations, checksums and the SBOM are made for every release file as before.
- `CODE_SIGNING.md` lists the beta list next to `releases.json`.
- Not solved here (and not new): a signed list has no expiry, so a stale list replayed on the path could hide
  a newer release. HTTPS-only transport covers the path today. A `generated` time in the list and a refusal of an
  older one is a follow-up.

### 3.7 Cadence (suggestions for the maintainer)

| Item | Suggestion |
|---|---|
| Update check | Unchanged: at most once a day, opt-in |
| Stable | When a milestone or a set of fixes is ready, not on a calendar; roughly every 6–10 weeks while the program is changing, longer later |
| Beta | One or two betas before each minor or major stable release (`X.Y.0-beta.1`, then `-rc.1` if the beta needed fixes); a soak of about a week without a new high-priority issue before the stable tag |
| Patch releases | `X.Y.Z` fixes go straight to stable, without a beta, when the fix is small and tested |
| Limit | Each release needs a SignPath approval and a person to publish the draft, so betas should be few |

### 3.8 Privacy and documents

A beta user's check requests `…/releases/download/update-channels/releases-beta.json` instead of
`…/releases/latest/download/releases.json`. GitHub therefore can tell which list was asked for. This is the
only difference; the host, the fixed User-Agent and the opt-in are the same, and nothing new is sent. `PRIVACY.md`
says so in the update-check row. The first-start question and its text are unchanged.

### 3.9 What the maintainer must decide

- Confirm two channels, named `stable` and `beta`.
- The cadence in §3.7 (the table is a suggestion).
- Whether to start the Flathub `beta` branch when Flathub is done.
- Publish the first `update-channels` release by publishing the first beta; there is nothing to create by hand
  (the workflow creates it). Until a first beta exists, the beta channel gets a 404, reported as a failed check.
  Also: until the first stable release exists, the stable address 404s as before.

## 4. Tests

- `fluut_core::update` (`update/tests.rs`): channel names and URLs; the signature sits next to each list;
  stable never offers a pre-release even to a pre-release run; beta offers pre-releases and the stable
  release after them; numeric identifiers compare as numbers; a stable user who switches to beta; no
  older pre-release than the running release; a list of the wrong channel is refused; the beta check requests exactly the
  two beta URLs.
- `fluut_core::settings`: default `stable`; the choice survives a save; an old file and an unknown value give `stable`
  and keep the rest.
- `fluut-app` `updates::tests`: the check gets the chosen channel; a channel change clears the result and is saved.
- `fluut-cli`: the saved channel is used unless `--channel` is given; a bad channel is refused (status 2) before any
  request.
- `xtask`: pre-release identifiers order by SemVer; the stable manifest refuses a pre-release; the beta manifest keeps both
  kinds newest first; a previous list of another channel is refused; unknown channels are refused.
- Workflows (`release.yml`, `channels.yml`) cannot be run locally. They are checked as YAML; the first beta tag
  is their real test (see the plan's manual check).

## 5. Follow-ups

- The first beta release as the end-to-end test of `channels.yml` (plan, last item).
- Flathub `beta` branch (§3.5).
- Expiry of a signed list (§3.6).
- A test that lists every URL constant and fails on a new host (§2.4 item 7); worth adding when the
  second network feature arrives.
