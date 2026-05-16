from __future__ import annotations

import logging

from homeassistant.components.select import SelectEntity
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
        new_entities: list[SelectEntity] = []
        for client_id in coordinator.data.clients:
            if client_id not in known_clients:
                known_clients.add(client_id)
                new_entities.append(SoniumClientGroupSelect(coordinator, client_id))
        if new_entities:
            async_add_entities(new_entities)

    _async_add_new_entities()
    entry.async_on_unload(coordinator.async_add_listener(_async_add_new_entities))


class SoniumClientGroupSelect(SoniumEntity, SelectEntity):
    """Select which group (zone) this client belongs to."""

    _attr_name = "Zone"
    _attr_icon = "mdi:speaker-multiple"

    def __init__(self, coordinator: SoniumCoordinator, client_id: str) -> None:
        super().__init__(coordinator)
        self._client_id = client_id
        self._attr_unique_id = f"{coordinator.entry_id}_client_{client_id}_group"

    @property
    def device_info(self) -> DeviceInfo:
        return self.client_device_info(self._client_id)

    @property
    def available(self) -> bool:
        return super().available and self._client_id in self.coordinator.data.clients

    @property
    def options(self) -> list[str]:
        return [g.name for g in self.coordinator.data.groups.values()]

    @property
    def current_option(self) -> str | None:
        client = self.coordinator.data.clients.get(self._client_id)
        if client is None:
            return None
        group = self.coordinator.data.groups.get(client.group_id)
        return group.name if group else None

    async def async_select_option(self, option: str) -> None:
        group = next(
            (g for g in self.coordinator.data.groups.values() if g.name == option),
            None,
        )
        if group is None:
            _LOGGER.warning("Group '%s' not found", option)
            return
        await self.coordinator.api.set_client_group(self._client_id, group.id)
        await self.coordinator.async_request_refresh()
