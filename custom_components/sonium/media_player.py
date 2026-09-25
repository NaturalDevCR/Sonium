from __future__ import annotations

import logging
from typing import Any

from homeassistant.components import media_source
from homeassistant.components.media_player import (
    ATTR_MEDIA_EXTRA,
    BrowseMedia,
    MediaPlayerEntity,
    MediaPlayerEntityFeature,
    MediaPlayerState,
    MediaType,
    async_process_play_media_url,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers import entity_registry as er
from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .api import CannotConnect, InvalidAuth, SoniumApiError
from .const import DOMAIN
from .coordinator import SoniumCoordinator
from .entity import SoniumEntity
from .group_mute import GroupMuteError, aggregate_group_mute, async_set_group_mute

_LOGGER = logging.getLogger(__name__)

# Percentage points for volume_up / volume_down.
VOLUME_STEP = 5

_PLAYBACK_FEATURES = (
    MediaPlayerEntityFeature.PLAY_MEDIA
    | MediaPlayerEntityFeature.MEDIA_ANNOUNCE
    | MediaPlayerEntityFeature.BROWSE_MEDIA
    | MediaPlayerEntityFeature.STOP
)
_VOLUME_FEATURES = (
    MediaPlayerEntityFeature.VOLUME_SET
    | MediaPlayerEntityFeature.VOLUME_MUTE
    | MediaPlayerEntityFeature.VOLUME_STEP
)


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


def _announce_volume(extra: dict[str, Any] | None) -> int | None:
    """Optional temporary volume from `extra: {volume: ...}`.

    Accepts 0.0–1.0 (like `volume_level`) or 0–100 (percent).
    """
    if not extra or extra.get("volume") is None:
        return None
    try:
        value = float(extra["volume"])
    except (TypeError, ValueError) as err:
        raise HomeAssistantError(f"Invalid announcement volume: {extra['volume']}") from err
    if value <= 1.0:
        value *= 100
    return max(0, min(100, int(round(value))))


class _SoniumPlayer(SoniumEntity, MediaPlayerEntity):
    """Behaviour shared by zone (group) and speaker (client) players."""

    async def _call(self, coro) -> Any:
        """Run an API call, turning failures into user-facing errors."""
        try:
            return await coro
        except SoniumApiError as err:
            raise HomeAssistantError(f"Sonium rejected the request: {err}") from err
        except (CannotConnect, InvalidAuth) as err:
            raise HomeAssistantError(f"Cannot reach the Sonium server: {err}") from err

    def _target(self) -> dict[str, list[str]]:
        raise NotImplementedError

    def _media_ids(self) -> set[str]:
        raise NotImplementedError

    async def async_play_media(
        self,
        media_type: MediaType | str,
        media_id: str,
        announce: bool | None = None,
        **kwargs: Any,
    ) -> None:
        """Play a URL, media-source item or TTS once, then resume the previous source."""
        if media_source.is_media_source_id(media_id):
            sourced = await media_source.async_resolve_media(
                self.hass, media_id, self.entity_id
            )
            media_id = sourced.url
        media_id = async_process_play_media_url(self.hass, media_id)

        volume = _announce_volume(kwargs.get(ATTR_MEDIA_EXTRA))
        _LOGGER.debug(
            "Playing %s on %s (announce=%s, volume=%s)",
            media_id,
            self.entity_id,
            announce,
            volume,
        )
        result = await self._call(
            self.coordinator.api.play_media(media_id, volume=volume, **self._target())
        )
        if result and result.get("id"):
            for client_id in result.get("client_ids", []):
                client = self.coordinator.data.clients.get(client_id)
                if client:
                    client.media_id = result["id"]
                    client.media_url = result.get("url")
            self.coordinator.async_update_listeners()

    async def async_browse_media(
        self,
        media_content_type: MediaType | str | None = None,
        media_content_id: str | None = None,
    ) -> BrowseMedia:
        return await media_source.async_browse_media(
            self.hass,
            media_content_id,
            content_filter=lambda item: item.media_content_type.startswith("audio/"),
        )

    async def async_media_stop(self) -> None:
        """Stop an announcement/URL playing here; the previous source resumes."""
        for media_id in self._media_ids():
            try:
                await self._call(self.coordinator.api.stop_media(media_id))
            except HomeAssistantError as err:
                # Already finished on the server — nothing to stop.
                _LOGGER.debug("Stopping media %s: %s", media_id, err)
        await self.coordinator.async_request_refresh()

    async def async_volume_up(self) -> None:
        if self.volume_level is not None:
            await self.async_set_volume_level(min(1.0, self.volume_level + VOLUME_STEP / 100))

    async def async_volume_down(self) -> None:
        if self.volume_level is not None:
            await self.async_set_volume_level(max(0.0, self.volume_level - VOLUME_STEP / 100))


class SoniumGroupMediaPlayer(_SoniumPlayer):
    """Represents a Sonium group (zone) as a media player."""

    _attr_supported_features = (
        MediaPlayerEntityFeature.SELECT_SOURCE
        | MediaPlayerEntityFeature.GROUPING
        | _VOLUME_FEATURES
        | _PLAYBACK_FEATURES
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
    def _clients(self) -> list:
        g = self._group
        if g is None:
            return []
        clients = self.coordinator.data.clients
        return [clients[cid] for cid in g.client_ids if cid in clients]

    def _target(self) -> dict[str, list[str]]:
        return {"group_ids": [self._group_id]}

    def _media_ids(self) -> set[str]:
        return {c.media_id for c in self._clients if c.media_id}

    @property
    def name(self) -> str | None:
        g = self._group
        return g.name if g else None

    @property
    def state(self) -> MediaPlayerState | None:
        g = self._group
        if g is None:
            return None
        if self._media_ids():
            return MediaPlayerState.PLAYING
        stream = self.coordinator.data.streams.get(g.stream_id)
        if stream and stream.status == "playing":
            return MediaPlayerState.PLAYING
        return MediaPlayerState.IDLE

    @property
    def media_content_id(self) -> str | None:
        urls = [c.media_url for c in self._clients if c.media_url]
        return urls[0] if urls else None

    @property
    def media_content_type(self) -> str | None:
        return MediaType.MUSIC if self.media_content_id else None

    @property
    def volume_level(self) -> float | None:
        """Average volume of the speakers in this zone."""
        clients = self._clients
        if not clients:
            return None
        return sum(c.volume for c in clients) / len(clients) / 100.0

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
    def is_volume_muted(self) -> bool | None:
        g = self._group
        if g is None:
            return None
        return aggregate_group_mute(g.client_ids, self.coordinator.data.clients)

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

    async def async_set_volume_level(self, volume: float) -> None:
        """Shift every speaker by the same amount, keeping their balance."""
        clients = self._clients
        current = self.volume_level
        if not clients or current is None:
            return
        delta = volume * 100 - current * 100
        for client in clients:
            new_volume = max(0, min(100, int(round(client.volume + delta))))
            if volume > 0 and new_volume == 0 and client.volume > 0:
                new_volume = 1
            await self._call(
                self.coordinator.api.set_volume(client.id, new_volume, client.muted)
            )
        await self.coordinator.async_request_refresh()

    async def async_select_source(self, source: str) -> None:
        stream = next(
            (s for s in self.coordinator.data.streams.values() if s.name == source),
            None,
        )
        if stream is None:
            raise HomeAssistantError(f"Sonium stream '{source}' not found")
        await self._call(self.coordinator.api.set_group_stream(self._group_id, stream.id))
        await self.coordinator.async_request_refresh()

    async def async_mute_volume(self, mute: bool) -> None:
        g = self._group
        if g is None:
            return
        try:
            await async_set_group_mute(
                g.client_ids,
                self.coordinator.data.clients,
                self.coordinator.api.set_volume,
                mute,
            )
        except GroupMuteError as err:
            _LOGGER.error("Group %s mute update failed: %s", self._group_id, err)
            raise HomeAssistantError(str(err)) from err
        finally:
            await self.coordinator.async_request_refresh()

    async def async_join_players(self, group_members: list[str]) -> None:
        """Move listed speaker entities into this zone."""
        registry = er.async_get(self.hass)
        prefix = f"{self.coordinator.entry_id}_client_"
        for entity_id in group_members:
            entry = registry.async_get(entity_id)
            if entry and entry.platform == DOMAIN and entry.unique_id.startswith(prefix):
                client_id = entry.unique_id[len(prefix):]
                await self._call(
                    self.coordinator.api.set_client_group(client_id, self._group_id)
                )
        await self.coordinator.async_request_refresh()

    async def async_unjoin_player(self) -> None:
        """A zone cannot leave itself; unjoin the individual speakers instead."""
        raise HomeAssistantError(
            "Unjoin a speaker entity to move it out of this Sonium zone"
        )


class SoniumClientMediaPlayer(_SoniumPlayer):
    """Represents a Sonium client (speaker) as a media player."""

    _attr_supported_features = (
        _VOLUME_FEATURES
        | MediaPlayerEntityFeature.SELECT_SOURCE
        | MediaPlayerEntityFeature.GROUPING
        | _PLAYBACK_FEATURES
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

    def _target(self) -> dict[str, list[str]]:
        return {"client_ids": [self._client_id]}

    def _media_ids(self) -> set[str]:
        client = self._client
        return {client.media_id} if client and client.media_id else set()

    @property
    def state(self) -> MediaPlayerState | None:
        client = self._client
        if client is None:
            return None
        if client.status == "disconnected":
            return MediaPlayerState.OFF
        if client.media_id:
            return MediaPlayerState.PLAYING
        group = self.coordinator.data.groups.get(client.group_id)
        if group is None:
            return MediaPlayerState.IDLE
        stream = self.coordinator.data.streams.get(group.stream_id)
        if stream and stream.status == "playing":
            return MediaPlayerState.PLAYING
        return MediaPlayerState.IDLE

    @property
    def media_content_id(self) -> str | None:
        client = self._client
        return client.media_url if client else None

    @property
    def media_content_type(self) -> str | None:
        return MediaType.MUSIC if self.media_content_id else None

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
        await self._call(
            self.coordinator.api.set_volume(
                self._client_id, int(round(volume * 100)), client.muted
            )
        )
        await self.coordinator.async_request_refresh()

    async def async_mute_volume(self, mute: bool) -> None:
        client = self._client
        if client is None:
            return
        await self._call(self.coordinator.api.set_volume(self._client_id, client.volume, mute))
        await self.coordinator.async_request_refresh()

    async def async_select_source(self, source: str) -> None:
        """Move this client to the selected group."""
        group = next(
            (g for g in self.coordinator.data.groups.values() if g.name == source),
            None,
        )
        if group is None:
            raise HomeAssistantError(f"Sonium zone '{source}' not found")
        await self._call(self.coordinator.api.set_client_group(self._client_id, group.id))
        await self.coordinator.async_request_refresh()

    async def async_join_players(self, group_members: list[str]) -> None:
        """Move the given speakers into this speaker's zone (this entity leads)."""
        client = self._client
        if client is None:
            return
        registry = er.async_get(self.hass)
        prefix = f"{self.coordinator.entry_id}_client_"
        for entity_id in group_members:
            entry = registry.async_get(entity_id)
            if entry and entry.platform == DOMAIN and entry.unique_id.startswith(prefix):
                other_client_id = entry.unique_id[len(prefix):]
                if other_client_id != self._client_id:
                    await self._call(
                        self.coordinator.api.set_client_group(other_client_id, client.group_id)
                    )
        await self.coordinator.async_request_refresh()

    async def async_unjoin_player(self) -> None:
        """Move this client to the default group."""
        await self._call(self.coordinator.api.set_client_group(self._client_id, "default"))
        await self.coordinator.async_request_refresh()
