# Homebrew Tap Automation

Automated Homebrew formula updates when stable releases are published. This is the canonical setup and renewal guide for the publishing credential.

## Credential and destination

| Item | Configuration |
| --- | --- |
| Workflow | [`release.yml`](../.github/workflows/release.yml), `update-homebrew` job |
| Secret location | **`branchbox/branchbox` → Settings → Secrets and variables → Actions → Repository secrets** |
| Secret name | `HOMEBREW_TAP_TOKEN` |
| Write destination | `branchbox/homebrew-tap`, `Formula/branchbox.rb` on its default branch |
| Credential type | Fine-grained personal access token (PAT) |
| Resource owner | `branchbox` organization |
| Repository access | Only select repositories → `homebrew-tap` |
| Repository permissions | Contents: Read and write; Metadata: Read-only (automatically included) |

The job checks out the tap using this PAT and pushes the formula commit. Its `permissions: contents: read` setting governs the workflow's built-in `GITHUB_TOKEN`; the PAT supplies the separate cross-repository write permission. The publishing PAT needs no Actions, Secrets, Administration, or Workflows permissions and no write access to `branchbox/branchbox`.

The PAT's **personal issuer** is the GitHub account that generated it; the resource owner above is the organization whose repository it can access. Repository configuration and the configured git author `branchbox-release-bot` do not identify that issuer. Check the issuer's signed-in account settings before renewal. GitHub's [PAT documentation](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens) describes ownership and repository restrictions.

Some historical setup documents in the tap recommend a classic token with broad `repo` scope. Use the selected-repository fine-grained configuration above instead.

## Renew or replace the token

PAT issuance and renewal require human interaction in GitHub. The helper below updates the existing secret after you obtain a new value; it does not create, regenerate, revoke, or extend tokens.

1. Open [Settings → Developer settings → Personal access tokens → Fine-grained tokens](https://github.com/settings/personal-access-tokens) in the issuing account. Locate the Homebrew publishing entry, checking its name, resource owner, selected repository, permissions, approval status, and last-use information. A fine-grained token settings URL identifies the interface, not proof that a particular Actions secret contains that PAT.
2. Inspect the exact expiration date/time and displayed timezone. Convert it to your operating timezone and record it privately. An email saying “expires in one day” is not an exact cutoff; repository secret timestamps do not establish expiry. If GitHub shows only a date, treat the exact cutoff as unverified and complete renewal before that date.
3. Choose a finite expiry allowed by [organization policy](https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/setting-a-personal-access-token-policy-for-your-organization), and keep the table's access unchanged. For a first setup, choose **Generate new token**, with a descriptive name such as `Homebrew Tap Updates`. For renewal, either regenerate the existing entry in GitHub, or create a replacement with the same access. **Regeneration invalidates the old value.** A separate replacement lets the previous working token remain available during the handoff; retire it manually only after confirming the replacement works and checking any other consumers.
4. Resolve any required organization approval before switching the secret. A pending token can read public resources but cannot publish. Keep the new value private and available only for the hidden terminal prompt in the next step. Do not paste it into chat, shell commands, logs, `.env` files, or repository files.
5. Use the helper's explicit `--apply` mode below to replace the existing secret, or update `HOMEBREW_TAP_TOKEN` directly in the [source repository's Actions secrets](https://github.com/branchbox/branchbox/settings/secrets/actions). For first setup, create the repository secret in that UI; the helper refuses to create a missing secret. Do not put it in the tap's secrets, Dependabot secrets, a repository variable, or an environment secret.

Creating or regenerating a PAT does not automatically replace its old value in Actions. The separate operator identity used by `gh` needs permission to manage repository Actions secrets in `branchbox/branchbox`; keep that privilege off the publishing PAT.

## Local helper

Run from the CLI repository root with Python 3. The default mode is an offline plan and requires neither a token nor `gh`:

```bash
python3 -B -I scripts/renew-homebrew-token.py --dry-run
```

For metadata checks, use an existing authorized [GitHub CLI](https://cli.github.com/) login for `github.com`. Both `--check` and `--apply` conservatively require the operator to have repository administrator access to `branchbox/branchbox`; the operator's existing authentication also needs access to repository Actions secret metadata and updates. The helper does not log in, create credentials, or change access:

```bash
python3 -B -I scripts/renew-homebrew-token.py --check
```

`--check` uses the current `gh` operator identity to read repository/secret metadata. GitHub's [secret GET endpoint](https://docs.github.com/en/rest/actions/secrets#get-a-repository-secret) exposes the name and creation/update timestamps, never the secret value. This check cannot identify the backing PAT, its owner, permissions, expiry, approval, last use, or current validity. Historical successful release jobs establish historical use only.

Optionally check a newly obtained value's syntax in a real terminal, without network access or writing a secret:

```bash
python3 -B -I scripts/renew-homebrew-token.py --validate
```

The hidden prompt checks the expected fine-grained PAT format only. It does not authenticate with the PAT or prove its scope or validity.

When ready to replace the live repository secret, run this command yourself in a private terminal:

```bash
python3 -B -I scripts/renew-homebrew-token.py --apply
```

Review the fixed target and operator identity, explicitly confirm the update, then paste the PAT at the hidden prompt. The helper supplies the value to `gh secret set` through standard input; it accepts no token argument and writes no token file. [GitHub CLI encrypts the value before submission](https://cli.github.com/manual/gh_secret_set). Do not enable debug output or terminal recording while entering credentials. Cancellation before submission leaves the secret unchanged.

After a successful update, run `--check` again and inspect the secret metadata. A successful set response plus metadata is not a read-back verification of the stored value or a publishing test. If submission fails or its outcome is uncertain, inspect metadata before deciding whether to retry; GitHub cannot return the previous value for recovery. Do not dispatch a release or rerun a publishing workflow just to test renewal. Verify formula publication during the next separately authorized stable release, as described below.

The helper's regression tests use dummy data and a fake `gh`; they never update GitHub:

```bash
python3 -B -I scripts/tests/test_renew_homebrew_token.py -v
```

---

## How It Works

On stable release:
1. Download checksums from GitHub Release
2. Update `Formula/branchbox.rb` with new version and SHA256s
3. Commit to [homebrew-tap](https://github.com/branchbox/homebrew-tap)

Skips pre-releases (`-alpha`, `-beta`, `-rc`).

---

## Verification

After a release, check:

**GitHub Actions**: [Actions tab](https://github.com/branchbox/branchbox/actions) → `Update Homebrew Formula` job ✓

**Homebrew Tap**: [Commits](https://github.com/branchbox/homebrew-tap/commits/main) → Look for `chore: update formula to vX.Y.Z`

**Test Install**:
```bash
brew update && brew upgrade branchbox
branchbox --version  # Should show new version
```

---

## Troubleshooting

### Job fails with "could not read from remote repository"
Token issue. Check:
- The repository secret exists in `branchbox/branchbox`, not the tap
- The PAT has not expired, been revoked, or lost its issuer's repository access
- Resource owner is `branchbox`, selected repository is `homebrew-tap`, and Contents has write permission
- Organization approval and lifetime policy permit the PAT

Follow the renewal procedure above, then update the repository secret. A successful `--check` does not establish PAT validity.

### Job fails with "Failed to extract checksums"
Release missing `checksums.txt` or format incorrect.

Verify: `curl -fsSL https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/checksums.txt`

Expected format: `{sha256}  branchbox-{version}-{target}.tar.gz`

### Formula update fails
Formula structure changed. View [current formula](https://raw.githubusercontent.com/branchbox/homebrew-tap/main/Formula/branchbox.rb) and ensure it matches expected structure (see below).

<details>
<summary>Show expected formula structure</summary>

```ruby
class Branchbox < Formula
  desc "Distributed development environment orchestrator"
  homepage "https://github.com/branchbox/branchbox"
  version "X.Y.Z"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/branchbox/branchbox/releases/download/v#{version}/branchbox-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "..."  # Replace with macOS Intel checksum
    end

    on_arm do
      url "https://github.com/branchbox/branchbox/releases/download/v#{version}/branchbox-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "..."  # Replace with macOS ARM checksum
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/branchbox/branchbox/releases/download/v#{version}/branchbox-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "..."  # Replace with Linux Intel checksum
    end

    on_arm do
      url "https://github.com/branchbox/branchbox/releases/download/v#{version}/branchbox-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "..."  # Replace with Linux ARM checksum
    end
  end
end
```
</details>

---

## Advanced

<details>
<summary>Manual formula update</summary>

If automation fails:

```bash
git clone https://github.com/branchbox/homebrew-tap
cd homebrew-tap

# Update Formula/branchbox.rb with new version and checksums
# Get checksums: curl -fsSL https://github.com/branchbox/branchbox/releases/download/vX.Y.Z/checksums.txt

git commit -am "chore: update formula to vX.Y.Z"
git push
```
</details>

<details>
<summary>Rollback formula</summary>

Quick revert:
```bash
cd homebrew-tap
git revert HEAD && git push
```

Or point to previous version by editing `Formula/branchbox.rb`.
</details>

<details>
<summary>Monitoring setup</summary>

Enable workflow failure notifications:
- Repository Settings → Notifications → Enable "Actions"
- Optional: [Slack integration](https://github.com/marketplace/actions/slack-send)
</details>

---

## Security

- Keep the publishing PAT restricted to the single tap and the permissions listed above.
- Enter its value only through the hidden helper prompt or GitHub's secret editor; never pass secrets as command-line arguments.
- Track its expiry privately and plan renewal before the cutoff; choose a policy-compliant finite lifetime.
- Never retrieve a token value from local credential stores for diagnostics or include it in support output.
- Secret updates and PAT lifecycle operations remain explicit operator actions; this helper does not trigger releases.

---

## Reference

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Feature Spec](../features/completed/homebrew-tap-distribution.md)
- Issues: [branchbox/homebrew-tap](https://github.com/branchbox/homebrew-tap/issues)
