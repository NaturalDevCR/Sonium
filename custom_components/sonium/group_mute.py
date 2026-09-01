"""Helpers for coordinating mute state across a Sonium group."""

from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable, Mapping, Sequence
from typing import Any


class GroupMuteError(Exception):
    """Raised when one or more group members could not be muted."""

    def __init__(self, failed_client_ids: Sequence[str]) -> None:
        self.failed_client_ids = tuple(failed_client_ids)
        members = ", ".join(self.failed_client_ids)
        super().__init__(f"Failed to update mute state for Sonium clients: {members}")


def aggregate_group_mute(
    client_ids: Sequence[str], clients: Mapping[str, Any]
) -> bool | None:
    """Return the group's mute state, or ``None`` when no member is known.

    A group is considered muted only when every known member is muted.  A
    mixed group therefore reports unmuted, which keeps the HA boolean state
    conservative and lets the user explicitly mute the whole group.
    """
    states = [
        clients[client_id].muted
        for client_id in dict.fromkeys(client_ids)
        if client_id in clients
    ]
    if not states:
        return None
    return all(states)


async def async_set_group_mute(
    client_ids: Sequence[str],
    clients: Mapping[str, Any],
    set_volume: Callable[[str, int, bool], Awaitable[None]],
    mute: bool,
) -> None:
    """Set mute for all known members while preserving each volume.

    All requests are attempted so one unavailable client does not prevent the
    remaining speakers from receiving the command.  Callers get the complete
    list of failed client IDs after the fan-out finishes.
    """
    targets = [
        (client_id, clients[client_id].volume)
        for client_id in dict.fromkeys(client_ids)
        if client_id in clients
    ]
    results = await asyncio.gather(
        *(set_volume(client_id, volume, mute) for client_id, volume in targets),
        return_exceptions=True,
    )
    failed_client_ids: list[str] = []
    for (client_id, _), result in zip(targets, results):
        if isinstance(result, asyncio.CancelledError):
            raise result
        if isinstance(result, Exception):
            failed_client_ids.append(client_id)
        elif isinstance(result, BaseException):
            raise result
    if failed_client_ids:
        raise GroupMuteError(failed_client_ids)
