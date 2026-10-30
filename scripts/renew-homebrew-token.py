#!/usr/bin/env python3
"""Prepare or explicitly update the BranchBox Homebrew Actions secret locally.

Uses only the Python standard library and an already authenticated GitHub CLI.
PAT generation/regeneration remains a manual operation in GitHub settings.
"""

import argparse
import getpass
import json
import os
import re
import subprocess
import sys
import warnings

SOURCE_REPO = "branchbox/branchbox"
TAP_REPO = "branchbox/homebrew-tap"
SECRET_NAME = "HOMEBREW_TAP_TOKEN"
CONFIRMATION = f"update {SOURCE_REPO} {SECRET_NAME}"


class RenewalError(Exception):
    """An operator-facing failure that never contains credential material."""


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        # Do not echo unknown arguments: someone may accidentally paste a PAT.
        self.print_usage(sys.stderr)
        self.exit(
            2,
            "Error: Invalid options. Use --help; never pass credentials as arguments.\n",
        )


def gh_environment():
    """Keep the operator's gh authentication, but disable debugging and prompts."""
    env = os.environ.copy()
    for name in ("GH_DEBUG", "DEBUG", "GH_FORCE_TTY", "GH_PAGER", "PAGER"):
        env.pop(name, None)
    env.update(GH_PROMPT_DISABLED="1", GH_HOST="github.com", NO_COLOR="1")
    return env


def run_gh(arguments, *, secret=None):
    # The replacement PAT travels only through stdin; never argv or environment.
    # cause-withheld: gh stderr/stdout might echo credential input or debug data.
    try:
        result = subprocess.run(
            ["gh", *arguments],
            input=secret,
            stdin=subprocess.DEVNULL if secret is None else None,
            text=True,
            capture_output=True,
            env=gh_environment(),
            timeout=60,
            check=False,
        )
    except FileNotFoundError:
        raise RenewalError("Install GitHub CLI (gh) before using this mode.") from None
    except (OSError, UnicodeError, subprocess.TimeoutExpired):
        if secret is not None:
            raise RenewalError(
                "Secret update outcome is uncertain. Run --check before retrying; "
                "do not assume the old value is still stored."
            ) from None
        raise RenewalError(
            "GitHub metadata request failed; check gh access and networking."
        ) from None
    if result.returncode:
        if secret is not None:
            raise RenewalError(
                "GitHub CLI did not confirm the secret update. Run --check before "
                "retrying; the update outcome may be uncertain."
            )
        raise RenewalError(
            "Cannot read required GitHub metadata. Check the existing gh login, "
            "repository access, and networking; no secret was changed."
        )
    return result.stdout if secret is None else ""


def read_metadata(endpoint):
    try:
        value = json.loads(run_gh(["api", "--hostname", "github.com", endpoint]))
    except (ValueError, TypeError):
        raise RenewalError(
            "GitHub returned invalid metadata; no update attempted."
        ) from None
    if not isinstance(value, dict):
        raise RenewalError("GitHub returned unexpected metadata; no update attempted.")
    return value


def check_access():
    actor = read_metadata("user").get("login")
    repo = read_metadata(f"repos/{SOURCE_REPO}")
    if not isinstance(actor, str) or not re.fullmatch(r"[A-Za-z0-9-]+", actor):
        raise RenewalError("Cannot establish the GitHub CLI operator identity.")
    if repo.get("full_name") != SOURCE_REPO:
        raise RenewalError(
            "GitHub returned a different source repository; refusing update."
        )
    permissions = repo.get("permissions")
    if not isinstance(permissions, dict) or permissions.get("admin") is not True:
        raise RenewalError(f"The gh operator needs admin access to {SOURCE_REPO}.")
    metadata = read_metadata(f"repos/{SOURCE_REPO}/actions/secrets/{SECRET_NAME}")
    if metadata.get("name") != SECRET_NAME:
        raise RenewalError(
            "Expected repository Actions secret is missing; refusing update."
        )
    print(f"GitHub CLI operator: {actor} (separate from the publishing PAT issuer)")
    print(f"Repository Actions secret: {SOURCE_REPO} / {SECRET_NAME}")
    for field in ("created_at", "updated_at"):
        value = metadata.get(field)
        # Only render timestamp-shaped values, never an unexpected API response.
        if not isinstance(value, str) or not re.fullmatch(r"[0-9T:Z.+-]+", value):
            raise RenewalError("Secret timestamp metadata is missing or invalid.")
        print(f"{field}: {value}")
    print(
        "Metadata cannot identify the stored PAT or prove its validity, permissions, or expiry."
    )
    return metadata


def require_local_terminal():
    if os.environ.get("CI") or os.environ.get("GITHUB_ACTIONS"):
        raise RenewalError("Credential entry is local-only; CI execution is refused.")
    if not (sys.stdin.isatty() and sys.stderr.isatty()):
        raise RenewalError(
            "Use a local terminal for hidden credential entry; pipes are refused."
        )


def read_replacement():
    # Raise instead of letting getpass fall back to an echoed stdin read.
    with warnings.catch_warnings():
        warnings.simplefilter("error", getpass.GetPassWarning)
        try:
            token = getpass.getpass("Paste the replacement fine-grained PAT (hidden): ")
        except getpass.GetPassWarning:
            raise RenewalError(
                "Hidden terminal input is unavailable; no update attempted."
            ) from None
    if not re.fullmatch(r"github_pat_[A-Za-z0-9_]{20,255}", token):
        raise RenewalError(
            "Expected a nonempty fine-grained PAT with no whitespace/control characters. "
            "No update attempted."
        )
    return token


def show_plan():
    print(f"Publishing destination: {TAP_REPO} / Formula/branchbox.rb")
    print(f"Secret location: github.com/{SOURCE_REPO}/settings/secrets/actions")
    print(f"Secret name: {SECRET_NAME}")
    print("PAT: fine-grained; resource owner branchbox; only homebrew-tap selected;")
    print("     Contents read/write and automatic Metadata read; no other permissions.")
    print(
        "Create/regenerate manually in GitHub settings and verify exact expiry/approval."
    )
    print("--check reads existing gh operator access and secret timestamps only.")
    print(
        "--validate checks hidden replacement input syntax locally without networking."
    )
    print(
        "--apply asks for confirmation and hidden input, then updates only this secret."
    )
    print("Dry run: no credentials read, network requests, or changes.")


def main(argv=None):
    parser = SafeParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument(
        "--dry-run", action="store_true", help="print offline plan (default)"
    )
    modes.add_argument(
        "--check", action="store_true", help="read non-secret GitHub metadata only"
    )
    modes.add_argument(
        "--validate", action="store_true", help="check hidden PAT syntax locally"
    )
    modes.add_argument(
        "--apply",
        action="store_true",
        help="explicitly update the existing Actions secret",
    )
    args = parser.parse_args(argv)
    try:
        if args.check:
            check_access()
        elif args.validate or args.apply:
            require_local_terminal()
            if args.apply:
                check_access()
                print(f"This will replace only {SOURCE_REPO} / {SECRET_NAME}.")
                print(
                    "The publishing PAT must be approved and limited to the tap as documented."
                )
                if input(f"Type '{CONFIRMATION}' to continue: ") != CONFIRMATION:
                    raise RenewalError(
                        "Confirmation did not match; no credential read or update attempted."
                    )
            token = read_replacement()
            try:
                if args.validate:
                    print(
                        "Syntax accepted locally. No network request or secret update made."
                    )
                    print(
                        "This does not validate the PAT's owner, access, expiry, or approval."
                    )
                else:
                    run_gh(
                        [
                            "secret",
                            "set",
                            SECRET_NAME,
                            "--repo",
                            f"github.com/{SOURCE_REPO}",
                            "--app",
                            "actions",
                        ],
                        secret=token,
                    )
                    print(
                        "GitHub CLI confirmed the secret update. No release was triggered."
                    )
                    print(
                        "Run --check to inspect timestamps; the stored value cannot be read back."
                    )
            finally:
                # Avoid persisting the PAT. Python cannot guarantee in-memory zeroization.
                token = None
        else:
            show_plan()
    except (RenewalError, EOFError, KeyboardInterrupt) as error:
        message = (
            str(error)
            if isinstance(error, RenewalError)
            else "Cancelled. Check metadata before retrying any interrupted update."
        )
        print(f"Error: {message}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
