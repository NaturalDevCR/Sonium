from __future__ import annotations

import logging

from homeassistant.components.media_player import (
    MediaPlayerEntity,
    MediaPlayerEntityFeature,
    MediaPlayerState,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers import entity_registry as er
from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import DOMAIN
from .coordinator import SoniumCoordinator
from .entity import SoniumEntity

_LOGGER = logging.getLogger(__name__)


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    coordinator: SoniumCoordinator = entry.runtime_data
    known_groups: set[str] = set()
    known_clients: set[str] = set()

    @callback
    def _async_add_new_entities() -> None:
        new_entities: list[MediaPlayerEntity] = []
        for group_id in coordinator.data.groups:
            if group_id not in known_groups:
                known_groups.add(group_id)
                new_entities.append(SoniumGroupMediaPlayer(coordinator, group_id))
        for client_id in coordinator.data.clients:
            if client_id not in known_clients:
                known_clients.add(client_id)
                new_entities.append(SoniumClientMediaPlayer(coordinator, client_id))
        if new_entities:
            async_add_entities(new_entities)

    _async_add_new_entities()
    entry.async_on_unload(coordinator.async_add_listener(_async_add_new_entities))


class SoniumGroupMediaPlayer(SoniumEntity, MediaPlayerEntity):
    """Represents a Sonium group (zone) as a media player."""

    _attr_supported_features = (
        MediaPlayerEntityFeature.SELECT_SOURCE | MediaPlayerEntityFeature.GROUPING
    )

    def __init__(self, coordinator: SoniumCoordinator, group_id: str) -> None:
        super().__init__(coordinator)
        self._group_id = group_id
        self._attr_unique_id = f"{coordinator.entry_id}_group_{group_id}"
        self._attr_device_info = self.server_device_info

    @property
    def available(self) -> bool:
        return super().available and self._group_id in self.coordinator.data.groups

    @property
    def _group(self):
        return self.coordinator.data.groups.get(self._group_id)

    @property
    def name(self) -> str | None:
        g = self._group
        return g.name if g else None

    @property
    def state(self) -> MediaPlayerState | None:
        g = self._group
        if g is None:
            return None
        stream = self.coordinator.data.streams.get(g.stream_id)
        if stream and stream.status == "playing":
            return MediaPlayerState.PLAYING
        return MediaPlayerState.IDLE

    @property
    def source(self) -> str | None:
        g = self._group
        if g is None:
            return None
        stream = self.coordinator.data.streams.get(g.stream_id)
        return stream.name if stream else g.stream_id

    @property
    def source_list(self) -> list[str]:
        return [s.name for s in self.coordinator.data.streams.values()]

    @property
    def group_members(self) -> list[str]:
        g = self._group
        if g is None:
            return []
        registry = er.async_get(self.hass)
        members = []
        for client_id in g.client_ids:
            unique_id = f"{self.coordinator.entry_id}_client_{client_id}"
            entity_id = registry.async_get_entity_id("media_player", DOMAIN, unique_id)
            if entity_id:
                members.append(entity_id)
        return members

    async def async_select_source(self, source: str) -> None:
        stream = next(
            (s for s in self.coordinator.data.streams.values() if s.name == source),
            None,
        )
        if stream is None:
            _LOGGER.warning("Stream '%s' not found", source)
            return
        await self.coordinator.api.set_group_stream(self._group_id, stream.id)
        await self.coordinator.async_request_refresh()

    async def async_join_players(self, group_members: list[str]) -> None:
        """Move listed client entities to this group."""
        registry = er.async_get(self.hass)
        for entity_id in group_members:
            entry = registry.async_get(entity_id)
            if entry and entry.platform == DOMAIN:
                prefix = f"{self.coordinator.entry_id}_client_"
                if entry.unique_id.startswith(prefix):
                    client_id = entry.unique_id[len(prefix):]
                    await self.coordinator.api.set_client_group(client_id, self._group_id)
        await self.coordinator.async_request_refresh()

    async def async_unjoin_player(self) -> None:
        pass


class SoniumClientMediaPlayer(SoniumEntity, MediaPlayerEntity):
    """Represents a Sonium client (speaker) as a media player."""

    _attr_supported_features = (
        MediaPlayerEntityFeature.VOLUME_SET
        | MediaPlayerEntityFeature.VOLUME_MUTE
        | MediaPlayerEntityFeature.SELECT_SOURCE
        | MediaPlayerEntityFeature.GROUPING
    )
    _attr_name = None  # Use device name as entity name

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator)
        self._client_id = client_id
        self._attr_unique_id = f"{coordinator.entry_id}_client_{client_id}"

    @property
    def device_info(self) -> DeviceInfo:
        return self.client_device_info(self._client_id)

    @property
    def available(self) -> bool:
        return super().available and self._client_id in self.coordinator.data.clients

    @property
    def _client(self):
        return self.coordinator.data.clients.get(self._client_id)

    @property
    def state(self) -> MediaPlayerState | None:
        client = self._client
        if client is None:
            return None
        if client.status == "disconnected":
            return MediaPlayerState.OFF
        group = self.coordinator.data.groups.get(client.group_id)
        if group is None:
            return MediaPlayerState.IDLE
        stream = self.coordinator.data.streams.get(group.stream_id)
        if stream and stream.status == "playing":
            return MediaPlayerState.PLAYING
        return MediaPlayerState.IDLE

    @property
    def volume_level(self) -> float | None:
        client = self._client
        return client.volume / 100.0 if client else None

    @property
    def is_volume_muted(self) -> bool | None:
        client = self._client
        return client.muted if client else None

    @property
    def source(self) -> str | None:
        """Return the group name the client belongs to."""
        client = self._client
        if client is None:
            return None
        group = self.coordinator.data.groups.get(client.group_id)
        return group.name if group else None

    @property
    def source_list(self) -> list[str]:
        """Return all group names for zone selection."""
        return [g.name for g in self.coordinator.data.groups.values()]

    @property
    def group_members(self) -> list[str]:
        """Return entity IDs of all clients in the same group."""
        client = self._client
        if client is None:
            return []
        group = self.coordinator.data.groups.get(client.group_id)
        if group is None:
            return []
        registry = er.async_get(self.hass)
        members = []
        for cid in group.client_ids:
            unique_id = f"{self.coordinator.entry_id}_client_{cid}"
            entity_id = registry.async_get_entity_id("media_player", DOMAIN, unique_id)
            if entity_id:
                members.append(entity_id)
        return members

    async def async_set_volume_level(self, volume: float) -> None:
        client = self._client
        if client is None:
            return
        await self.coordinator.api.set_volume(
            self._client_id, int(round(volume * 100)), client.muted
        )
        await self.coordinator.async_request_refresh()

    async def async_mute_volume(self, mute: bool) -> None:
        client = self._client
        if client is None:
            return
        await self.coordinator.api.set_volume(self._client_id, client.volume, mute)
        await self.coordinator.async_request_refresh()

    async def async_select_source(self, source: str) -> None:
        """Move this client to the selected group."""
        group = next(
            (g for g in self.coordinator.data.groups.values() if g.name == source),
            None,
        )
        if group is None:
            _LOGGER.warning("Group '%s' not found", source)
            return
        await self.coordinator.api.set_client_group(self._client_id, group.id)
        await self.coordinator.async_request_refresh()

    async def async_join_players(self, group_members: list[str]) -> None:
        """Move this client to the same group as the first member."""
        registry = er.async_get(self.hass)
        for entity_id in group_members:
            entry = registry.async_get(entity_id)
            if entry and entry.platform == DOMAIN:
                prefix = f"{self.coordinator.entry_id}_client_"
                if entry.unique_id.startswith(prefix):
                    other_client_id = entry.unique_id[len(prefix):]
                    other_client = self.coordinator.data.clients.get(other_client_id)
                    if other_client:
                        await self.coordinator.api.set_client_group(
                            self._client_id, other_client.group_id
                        )
                        await self.coordinator.async_request_refresh()
                        return

    async def async_unjoin_player(self) -> None:
        """Move this client to the default group."""
        await self.coordinator.api.set_client_group(self._client_id, "default")
        await self.coordinator.async_request_refresh()
