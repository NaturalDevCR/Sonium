from __future__ import annotations

import logging

from homeassistant.components.number import NumberEntity, NumberMode
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .coordinator import SoniumCoordinator
from .entity import SoniumEntity

_LOGGER = logging.getLogger(__name__)


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    coordinator: SoniumCoordinator = entry.runtime_data
    known_clients: set[str] = set()

    @callback
    def _async_add_new_entities() -> None:
        new_entities: list[NumberEntity] = []
        for client_id in coordinator.data.clients:
            if client_id not in known_clients:
                known_clients.add(client_id)
                new_entities.append(SoniumClientLatencyNumber(coordinator, client_id))
        if new_entities:
            async_add_entities(new_entities)

    _async_add_new_entities()
    entry.async_on_unload(coordinator.async_add_listener(_async_add_new_entities))


class SoniumClientLatencyNumber(SoniumEntity, NumberEntity):
    """Adjust the audio latency offset for a client in milliseconds.

    Positive values delay playback (useful for slow Bluetooth speakers).
    Negative values advance playback (rarely needed).
    """

    _attr_name = "Latency Offset"
    _attr_native_unit_of_measurement = "ms"
    _attr_native_min_value = -1000.0
    _attr_native_max_value = 1000.0
    _attr_native_step = 1.0
    _attr_mode = NumberMode.BOX
    _attr_icon = "mdi:timer-sync-outline"

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator)
        self._client_id = client_id
        self._attr_unique_id = f"{coordinator.entry_id}_client_{client_id}_latency"

    @property
    def device_info(self) -> DeviceInfo:
        return self.client_device_info(self._client_id)

    @property
    def available(self) -> bool:
        return super().available and self._client_id in self.coordinator.data.clients

    @property
    def native_value(self) -> float | None:
        client = self.coordinator.data.clients.get(self._client_id)
        return float(client.latency_ms) if client else None

    async def async_set_native_value(self, value: float) -> None:
        await self.coordinator.api.set_latency(self._client_id, int(value))
        await self.coordinator.async_request_refresh()
