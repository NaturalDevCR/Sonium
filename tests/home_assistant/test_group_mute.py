"""Tests for group-level mute coordination in the Home Assistant adapter."""

from __future__ import annotations

import asyncio
import importlib.util
from pathlib import Path
import unittest

_MODULE_PATH = (
    Path(__file__).resolve().parents[2]
    / "custom_components"
    / "sonium"
    / "group_mute.py"
)
_SPEC = importlib.util.spec_from_file_location("sonium_group_mute", _MODULE_PATH)
assert _SPEC and _SPEC.loader
_MODULE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MODULE)
GroupMuteError = _MODULE.GroupMuteError
aggregate_group_mute = _MODULE.aggregate_group_mute
async_set_group_mute = _MODULE.async_set_group_mute


class _Client:
    def __init__(self, volume: int, muted: bool) -> None:
        self.volume = volume
        self.muted = muted


class GroupMuteTests(unittest.IsolatedAsyncioTestCase):
    def test_aggregate_state_requires_all_known_members_muted(self) -> None:
        clients = {
            "a": _Client(35, True),
            "b": _Client(80, True),
        }
        self.assertTrue(aggregate_group_mute(["a", "b"], clients))
        clients["b"].muted = False
        self.assertFalse(aggregate_group_mute(["a", "b"], clients))

    def test_aggregate_state_is_unknown_without_known_members(self) -> None:
        self.assertIsNone(aggregate_group_mute(["missing"], {}))

    async def test_sets_mute_for_each_member_preserving_volume(self) -> None:
        clients = {
            "a": _Client(35, False),
            "b": _Client(80, True),
        }
        calls: list[tuple[str, int, bool]] = []

        async def set_volume(client_id: str, volume: int, muted: bool) -> None:
            calls.append((client_id, volume, muted))

        await async_set_group_mute(["a", "b"], clients, set_volume, True)

        self.assertCountEqual(calls, [("a", 35, True), ("b", 80, True)])

    async def test_attempts_all_members_and_reports_partial_failures(self) -> None:
        clients = {
            "a": _Client(35, False),
            "b": _Client(80, False),
            "c": _Client(20, False),
        }
        calls: list[str] = []

        async def set_volume(client_id: str, volume: int, muted: bool) -> None:
            calls.append(client_id)
            await asyncio.sleep(0)
            if client_id == "b":
                raise RuntimeError("client unavailable")

        with self.assertRaises(GroupMuteError) as context:
            await async_set_group_mute(["a", "b", "c"], clients, set_volume, True)

        self.assertCountEqual(calls, ["a", "b", "c"])
        self.assertEqual(context.exception.failed_client_ids, ("b",))
