#!/usr/bin/env python3
"""Static regression checks for the supported CI and tag-release shape."""

from __future__ import annotations

import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


class WorkflowContractTests(unittest.TestCase):
	def test_language_checks_keep_all_supported_lanes_and_triggers(self) -> None:
		workflow = (ROOT / ".github/workflows/language.yml").read_text(encoding="utf-8")
		for required in (
			"push:",
			"pull_request:",
			"merge_group:",
			"rust-check:",
			"swift-check:",
			"toml-check:",
			"runs-on: macos-26",
			"cargo make test-release",
		):
			self.assertIn(required, workflow)

	def test_release_permissions_and_jobs_are_minimal(self) -> None:
		workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
		self.assertIn("permissions:\n  contents: read", workflow)
		self.assertEqual(workflow.count("contents: write"), 1)
		self.assertEqual(workflow.count("runs-on: macos-26"), 1)
		self.assertNotIn("workflow_dispatch", workflow)
		self.assertNotIn("Release Preparation", workflow)
		self.assertNotIn("Release Dry Run", workflow)
		self.assertIn("needs: [validate-release, build-macos]", workflow)
		self.assertIn("environment:\n      name: release", workflow)
		for secret in (
			"APPLE_CERTIFICATE_P12_BASE64",
			"APPLE_CERTIFICATE_PASSWORD",
			"APPLE_SIGNING_IDENTITY",
			"SPARKLE_PRIVATE_ED_KEY",
		):
			self.assertIn(f"secrets.{secret}", workflow)
		self.assertNotIn("RSNAP_SPARKLE_PRIVATE_ED_KEY", workflow)
		self.assertNotIn("APPLE_DEVELOPER_ID_APPLICATION", workflow)
		self.assertNotIn("APPLE_NOTARY_", workflow)
		self.assertNotIn("notarytool", workflow)
		self.assertIn("retention-days: 7", workflow)

	def test_canonical_repository_has_no_stale_owner_reference(self) -> None:
		result = subprocess.run(
			[
				"git",
				"-C",
				str(ROOT),
				"grep",
				"-n",
				"acgxv/rsnap",
				"--",
				":!scripts/release/tests/test_workflow_contract.py",
			],
			capture_output=True,
			text=True,
		)
		self.assertEqual(result.returncode, 1, result.stdout)


if __name__ == "__main__":
	unittest.main()
