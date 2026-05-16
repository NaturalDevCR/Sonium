from __future__ import annotations

from homeassistant.components.binary_sensor import (
    BinarySensorDeviceClass,
    BinarySensorEntity,
)
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .coordinator import SoniumCoordinator
from .entity import SoniumEntity


async def async_setup_entry(
    hass: HomeAssistant,
    entry: ConfigEntry,
    async_add_entities: AddEntitiesCallback,
) -> None:
    coordinator: SoniumCoordinator = entry.runtime_data
    known_clients: set[str] = set()

    @callback
    def _async_add_new_entities() -> None:
        new_entities: list[BinarySensorEntity] = []
        for client_id in coordinator.data.clients:
            if client_id not in known_clients:
                known_clients.add(client_id)
                new_entities.append(SoniumClientConnectedSensor(coordinator, client_id))
        if new_entities:
            async_add_entities(new_entities)

    _async_add_new_entities()
    entry.async_on_unload(coordinator.async_add_listener(_async_add_new_entities))


class SoniumClientConnectedSensor(SoniumEntity, BinarySensorEntity):
    """Reports whether a Sonium client is currently connected."""

    _attr_device_class = BinarySensorDeviceClass.CONNECTIVITY
    _attr_name = "Connected"

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator)
        self._client_id = client_id
        self._attr_unique_id = f"{coordinator.entry_id}_client_{client_id}_connected"

    @property
    def device_info(self) -> DeviceInfo:
        return self.client_device_info(self._client_id)

    @property
    def available(self) -> bool:
        return super().available and self._client_id in self.coordinator.data.clients

    @property
    def is_on(self) -> bool | None:
        client = self.coordinator.data.clients.get(self._client_id)
        if client is None:
            return None
        return client.status == "connected"

    @property
    def extra_state_attributes(self) -> dict:
        client = self.coordinator.data.clients.get(self._client_id)
        if client is None:
            return {}
        return {
            "remote_addr": client.remote_addr,
            "client_name": client.client_name,
            "os": client.os,
            "arch": client.arch,
        }
