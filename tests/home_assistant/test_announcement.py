"""Contract tests for the Home Assistant announcement adapter.

These tests deliberately exercise the Python payload boundary without importing
Home Assistant, so the Sonium REST contract remains verifiable in lightweight
CI as well as inside Home Assistant.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


_MODULE_PATH = (
    Path(__file__).resolve().parents[2]
    / "custom_components"
    / "sonium"
    / "announcement.py"
)
_SPEC = importlib.util.spec_from_file_location("sonium_announcement", _MODULE_PATH)
assert _SPEC and _SPEC.loader
_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MODULE)
AnnouncementValidationError = _MODULE.AnnouncementValidationError
announcement_options_from_media_kwargs = _MODULE.announcement_options_from_media_kwargs
build_announcement_intent = _MODULE.build_announcement_intent


class AnnouncementIntentTests(unittest.TestCase):
    def test_builds_the_versioned_server_contract(self) -> None:
        intent = build_announcement_intent(
            source="https://media.example.test/doorbell.ogg",
            target_groups=["kitchen", "bedroom"],
            idempotency_key="doorbell-20260901-001",
            priority="announcement",
            attenuation_db=-18,
            attack_ms=25,
            release_ms=100,
            max_duration_ms=5_000,
            resume=True,
            now_ms=1_000_000,
        )

        self.assertEqual(intent["version"], 1)
        self.assertEqual(intent["target_groups"], ["kitchen", "bedroom"])
        self.assertEqual(intent["source"], {
            "kind": "uri",
            "uri": "https://media.example.test/doorbell.ogg",
        })
        self.assertEqual(intent["duck"], {
            "attenuation_db": -18.0,
            "attack_ms": 25,
            "release_ms": 100,
        })
        self.assertEqual(intent["resume"], "resume_previous")
        self.assertGreater(intent["expires_at_ms"], 1_000_000)

    def test_rejects_invalid_ducking_before_submitting(self) -> None:
        with self.assertRaisesRegex(AnnouncementValidationError, "attenuation"):
            build_announcement_intent(
                source="https://media.example.test/doorbell.ogg",
                target_groups=["kitchen"],
                idempotency_key="doorbell-1",
                attenuation_db=-61,
                now_ms=1_000_000,
            )

        with self.assertRaisesRegex(AnnouncementValidationError, "attack_ms"):
            build_announcement_intent(
                source="https://media.example.test/doorbell.ogg",
                target_groups=["kitchen"],
                idempotency_key="doorbell-1",
                attack_ms=True,
                now_ms=1_000_000,
            )

    def test_rejects_non_http_media_source_before_submitting(self) -> None:
        with self.assertRaisesRegex(AnnouncementValidationError, "http"):
            build_announcement_intent(
                source="file:///tmp/doorbell.ogg",
                target_groups=["kitchen"],
                idempotency_key="doorbell-1",
                now_ms=1_000_000,
            )

    def test_rejects_duplicate_or_empty_group_targets(self) -> None:
        with self.assertRaisesRegex(AnnouncementValidationError, "unique"):
            build_announcement_intent(
                source="https://media.example.test/doorbell.ogg",
                target_groups=["kitchen", "kitchen"],
                idempotency_key="doorbell-1",
                now_ms=1_000_000,
            )

    def test_recognizes_home_assistant_announcement_metadata(self) -> None:
        options = announcement_options_from_media_kwargs(
            {
                "announce": True,
                "extra": {
                    "priority": "emergency",
                    "duck": {"attenuation_db": -30, "attack_ms": 10},
                    "release_ms": 250,
                    "resume": False,
                },
            }
        )

        self.assertEqual(options["priority"], "emergency")
        self.assertEqual(options["attenuation_db"], -30)
        self.assertEqual(options["attack_ms"], 10)
        self.assertEqual(options["release_ms"], 250)
        self.assertFalse(options["resume"])

    def test_ignores_regular_media_playback_metadata(self) -> None:
        self.assertIsNone(announcement_options_from_media_kwargs({"extra": {}}))

        with self.assertRaisesRegex(AnnouncementValidationError, "at least one"):
            build_announcement_intent(
                source="https://media.example.test/doorbell.ogg",
                target_groups=[],
                idempotency_key="doorbell-1",
                now_ms=1_000_000,
            )
