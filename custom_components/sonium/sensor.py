from __future__ import annotations

import logging

from homeassistant.components.sensor import (
    SensorDeviceClass,
    SensorEntity,
    SensorStateClass,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .coordinator import SoniumCoordinator
from .entity import SoniumEntity

_LOGGER = logging.getLogger(__name__)

STREAM_STATUS_MAP = {
    "playing": "playing",
    "idle": "idle",
    "recovering": "recovering",
    "error": "error",
}


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    coordinator: SoniumCoordinator = entry.runtime_data
    known_streams: set[str] = set()
    known_clients: set[str] = set()

    @callback
    def _async_add_new_entities() -> None:
        new_entities: list[SensorEntity] = []
        for stream_id in coordinator.data.streams:
            if stream_id not in known_streams:
                known_streams.add(stream_id)
                new_entities.append(SoniumStreamStatusSensor(coordinator, stream_id))
        for client_id in coordinator.data.clients:
            if client_id not in known_clients:
                known_clients.add(client_id)
                new_entities.extend(
                    [
                        SoniumClientJitterSensor(coordinator, client_id),
                        SoniumClientBufferSensor(coordinator, client_id),
                        SoniumClientUnderrunSensor(coordinator, client_id),
                    ]
                )
        if new_entities:
            async_add_entities(new_entities)

    _async_add_new_entities()
    entry.async_on_unload(coordinator.async_add_listener(_async_add_new_entities))


class SoniumStreamStatusSensor(SoniumEntity, SensorEntity):
    """Stream playback status: playing | idle | error."""

    _attr_translation_key = "stream_status"

    def __init__(self, coordinator: SoniumCoordinator, stream_id: str) -> None:
        super().__init__(coordinator)
        self._stream_id = stream_id
        self._attr_unique_id = f"{coordinator.entry_id}_stream_{stream_id}_status"
        self._attr_device_info = self.server_device_info

    @property
    def available(self) -> bool:
        return super().available and self._stream_id in self.coordinator.data.streams

    @property
    def _stream(self):
        return self.coordinator.data.streams.get(self._stream_id)

    @property
    def name(self) -> str | None:
        s = self._stream
        return f"{s.name} Status" if s else None

    @property
    def native_value(self) -> str | None:
        s = self._stream
        return s.status if s else None

    @property
    def icon(self) -> str:
        s = self._stream
        if s is None:
            return "mdi:music-off"
        if s.status == "playing":
            return "mdi:music"
        if s.status == "error":
            return "mdi:music-off"
        return "mdi:music-note-off"

    @property
    def extra_state_attributes(self) -> dict:
        s = self._stream
        if s is None:
            return {}
        attributes = {"codec": s.codec, "format": s.format, "stream_id": s.id}
        if s.recovery:
            attributes.update(
                recovery_attempt=s.recovery.attempt,
                retry_in_ms=s.recovery.retry_in_ms,
            )
        return attributes


class _ClientHealthSensor(SoniumEntity, SensorEntity):
    """Base class for client health sensors."""

    _attr_state_class = SensorStateClass.MEASUREMENT

    def __init__(
        self, coordinator: SoniumCoordinator, client_id: str, key: str
    ) -> None:
        super().__init__(coordinator)
        self._client_id = client_id
        self._health_key = key
        self._attr_unique_id = f"{coordinator.entry_id}_client_{client_id}_{key}"

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
    def native_value(self) -> float | None:
        client = self._client
        if client is None or client.health is None:
            return None
        return getattr(client.health, self._health_key, None)


class SoniumClientJitterSensor(_ClientHealthSensor):
    """Client audio jitter in milliseconds."""

    _attr_native_unit_of_measurement = "ms"
    _attr_device_class = SensorDeviceClass.DURATION
    _attr_suggested_display_precision = 1

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator, client_id, "jitter_ms")
        self._attr_name = "Jitter"
        self._attr_icon = "mdi:sine-wave"


class SoniumClientBufferSensor(_ClientHealthSensor):
    """Client jitter buffer depth in milliseconds."""

    _attr_native_unit_of_measurement = "ms"
    _attr_device_class = SensorDeviceClass.DURATION
    _attr_suggested_display_precision = 1

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator, client_id, "buffer_depth_ms")
        self._attr_name = "Buffer Depth"
        self._attr_icon = "mdi:buffer"


class SoniumClientUnderrunSensor(_ClientHealthSensor):
    """Cumulative audio underrun count for the client."""

    _attr_state_class = SensorStateClass.TOTAL_INCREASING

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator, client_id, "underrun_count")
        self._attr_name = "Underruns"
        self._attr_icon = "mdi:alert-circle-outline"
