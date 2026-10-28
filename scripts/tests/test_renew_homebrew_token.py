"""Offline safety regressions. All credential input is dummy data."""

import contextlib
import getpass
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
import warnings
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "renew-homebrew-token.py"
SPEC = importlib.util.spec_from_file_location("renew_homebrew_token", SCRIPT)
renew = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(renew)
DUMMY = "github_pat_" + "DUMMY_FOR_OFFLINE_TESTS_" * 3
METADATA = {
    "name": renew.SECRET_NAME,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-02-01T00:00:00Z",
}


class RenewalTests(unittest.TestCase):
    def invoke(self, args, *, terminal=True, confirmation=None, token=DUMMY):
        out, err = io.StringIO(), io.StringIO()
        with (
            contextlib.redirect_stdout(out),
            contextlib.redirect_stderr(err),
            patch.object(renew.sys.stdin, "isatty", return_value=terminal),
            patch.object(err, "isatty", return_value=terminal, create=True),
            patch.dict(os.environ, {"CI": "", "GITHUB_ACTIONS": ""}),
            patch(
                "builtins.input", return_value=confirmation or renew.CONFIRMATION
            ) as prompt,
            patch.object(renew.getpass, "getpass", return_value=token) as hidden,
        ):
            status = renew.main(args)
        return status, out.getvalue() + err.getvalue(), prompt, hidden

    def test_default_and_explicit_dry_run_are_offline_without_credential_input(self):
        with patch.object(renew, "run_gh") as gh:
            for args in ([], ["--dry-run"]):
                status, output, prompt, hidden = self.invoke(args)
                self.assertEqual(status, 0)
                self.assertIn("Dry run:", output)
                self.assertIn(renew.SOURCE_REPO, output)
                self.assertIn(renew.TAP_REPO, output)
                prompt.assert_not_called()
                hidden.assert_not_called()
            gh.assert_not_called()

    def test_check_reads_only_explicit_metadata_endpoints(self):
        replies = [
            {"login": "operator"},
            {"full_name": renew.SOURCE_REPO, "permissions": {"admin": True}},
            METADATA,
        ]
        with patch.object(
            renew, "run_gh", side_effect=[json.dumps(r) for r in replies]
        ) as gh:
            status, output, prompt, hidden = self.invoke(["--check"])
        self.assertEqual(status, 0)
        self.assertIn("separate from the publishing PAT issuer", output)
        self.assertIn("cannot identify", output)
        self.assertEqual(
            [c.args[0] for c in gh.call_args_list],
            [
                ["api", "--hostname", "github.com", "user"],
                ["api", "--hostname", "github.com", "repos/branchbox/branchbox"],
                [
                    "api",
                    "--hostname",
                    "github.com",
                    "repos/branchbox/branchbox/actions/secrets/HOMEBREW_TAP_TOKEN",
                ],
            ],
        )
        prompt.assert_not_called()
        hidden.assert_not_called()

    def test_check_refuses_missing_admin_or_wrong_repo(self):
        for repository in (
            {"full_name": renew.SOURCE_REPO, "permissions": {"push": True}},
            {"full_name": "other/repository", "permissions": {"admin": True}},
        ):
            with patch.object(
                renew, "read_metadata", side_effect=[{"login": "operator"}, repository]
            ):
                status, _, _, hidden = self.invoke(["--apply"])
            self.assertEqual(status, 1)
            hidden.assert_not_called()

    def test_apply_passes_dummy_only_via_stdin_to_one_fixed_secret(self):
        with patch.object(renew, "check_access"), patch.object(renew, "run_gh") as gh:
            status, output, _, _ = self.invoke(["--apply"])
        self.assertEqual(status, 0)
        gh.assert_called_once_with(
            [
                "secret",
                "set",
                "HOMEBREW_TAP_TOKEN",
                "--repo",
                "github.com/branchbox/branchbox",
                "--app",
                "actions",
            ],
            secret=DUMMY,
        )
        self.assertNotIn(DUMMY, output)
        self.assertIn("No release was triggered", output)

    def test_confirmation_mismatch_never_reads_credential(self):
        with patch.object(renew, "check_access"), patch.object(renew, "run_gh") as gh:
            status, _, _, hidden = self.invoke(["--apply"], confirmation="no")
        self.assertEqual(status, 1)
        hidden.assert_not_called()
        gh.assert_not_called()

    def test_invalid_input_never_updates_or_echoes(self):
        for bad in (
            "",
            " ",
            "classic_dummy",
            DUMMY + "\n",
            DUMMY + "\r",
            DUMMY + "\x1b",
            DUMMY + " ",
        ):
            with (
                patch.object(renew, "check_access"),
                patch.object(renew, "run_gh") as gh,
            ):
                status, output, _, _ = self.invoke(["--apply"], token=bad)
            self.assertEqual(status, 1)
            if bad.strip():
                self.assertNotIn(bad, output)
            gh.assert_not_called()

    def test_validate_is_local_syntax_check_only(self):
        with patch.object(renew, "run_gh") as gh:
            status, output, prompt, _ = self.invoke(["--validate"])
        self.assertEqual(status, 0)
        self.assertNotIn(DUMMY, output)
        self.assertIn("does not validate", output)
        prompt.assert_not_called()
        gh.assert_not_called()

    def test_piped_credential_input_is_refused(self):
        for mode in ("--validate", "--apply"):
            with patch.object(renew, "run_gh") as gh:
                status, _, prompt, hidden = self.invoke([mode], terminal=False)
            self.assertEqual(status, 1)
            prompt.assert_not_called()
            hidden.assert_not_called()
            gh.assert_not_called()

    def test_ci_credential_entry_is_refused(self):
        with (
            patch.dict(os.environ, {"CI": "true"}),
            self.assertRaisesRegex(renew.RenewalError, "local-only"),
        ):
            renew.require_local_terminal()

    def test_hidden_input_never_falls_back_to_echo(self):
        def warn_instead_of_hidden_input(*_args):
            warnings.warn("Cannot control echo", getpass.GetPassWarning)
            return DUMMY

        with (
            patch.object(
                renew.getpass, "getpass", side_effect=warn_instead_of_hidden_input
            ),
            self.assertRaisesRegex(renew.RenewalError, "Hidden terminal input"),
        ):
            renew.read_replacement()

    def test_subprocess_never_receives_replacement_in_argv_or_environment(self):
        with (
            patch.dict(os.environ, {"GH_DEBUG": "api", "GH_FORCE_TTY": "1"}),
            patch.object(
                renew.subprocess,
                "run",
                return_value=subprocess.CompletedProcess([], 0, DUMMY, DUMMY),
            ) as run,
        ):
            self.assertEqual(
                renew.run_gh(["secret", "set", renew.SECRET_NAME], secret=DUMMY), ""
            )
        args, kwargs = run.call_args
        self.assertNotIn(DUMMY, args[0])
        self.assertNotIn(DUMMY, kwargs["env"].values())
        self.assertEqual(kwargs["input"], DUMMY)
        self.assertTrue(kwargs["capture_output"])
        self.assertNotIn("GH_DEBUG", kwargs["env"])
        self.assertNotIn("GH_FORCE_TTY", kwargs["env"])
        self.assertEqual(kwargs["env"]["GH_PROMPT_DISABLED"], "1")

    def test_child_error_output_is_withheld_and_update_uncertainty_is_reported(self):
        for failure in (
            subprocess.CompletedProcess([], 1, DUMMY, DUMMY),
            subprocess.TimeoutExpired("gh", 60, output=DUMMY, stderr=DUMMY),
        ):
            with patch.object(renew.subprocess, "run") as run:
                if isinstance(failure, BaseException):
                    run.side_effect = failure
                else:
                    run.return_value = failure
                with self.assertRaisesRegex(renew.RenewalError, "uncertain") as error:
                    renew.run_gh(["secret", "set", renew.SECRET_NAME], secret=DUMMY)
            self.assertNotIn(DUMMY, str(error.exception))

    def test_real_child_process_receives_dummy_through_stdin(self):
        with tempfile.TemporaryDirectory(prefix="branchbox-fake-gh-") as directory:
            fake = Path(directory) / "gh"
            fake.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                f"expected = {DUMMY!r}\n"
                "assert sys.argv[1:] == ['secret', 'set', 'HOMEBREW_TAP_TOKEN', "
                "'--repo', 'github.com/branchbox/branchbox', '--app', 'actions']\n"
                "assert sys.stdin.read() == expected\n"
                "print(expected)  # Deliberately unsafe child output must be withheld.\n"
                "print(expected, file=sys.stderr)\n",
                encoding="utf-8",
            )
            fake.chmod(0o700)
            with patch.dict(
                os.environ, {"PATH": directory + os.pathsep + os.environ["PATH"]}
            ):
                result = renew.run_gh(
                    [
                        "secret",
                        "set",
                        renew.SECRET_NAME,
                        "--repo",
                        "github.com/branchbox/branchbox",
                        "--app",
                        "actions",
                    ],
                    secret=DUMMY,
                )
            self.assertEqual(result, "")

    def test_metadata_parse_failures_and_missing_secret_abort(self):
        with (
            patch.object(renew, "run_gh", return_value="not json"),
            self.assertRaises(renew.RenewalError),
        ):
            renew.read_metadata("user")
        replies = [
            {"login": "operator"},
            {"full_name": renew.SOURCE_REPO, "permissions": {"admin": True}},
            {"name": "WRONG_SECRET"},
        ]
        with patch.object(renew, "read_metadata", side_effect=replies):
            status, _, _, hidden = self.invoke(["--apply"])
        self.assertEqual(status, 1)
        hidden.assert_not_called()

    def test_cli_rejects_token_arguments(self):
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                "-I",
                str(SCRIPT),
                "--token",
                "dummy-not-a-credential",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("dummy-not-a-credential", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
