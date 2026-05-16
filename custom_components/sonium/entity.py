from __future__ import annotations

from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.update_coordinator import CoordinatorEntity

from .const import DOMAIN
from .coordinator import SoniumCoordinator


class SoniumEntity(CoordinatorEntity[SoniumCoordinator]):
    _attr_has_entity_name = True

    def __init__(self, coordinator: SoniumCoordinator) -> None:
        super().__init__(coordinator)

    @property
    def server_device_info(self) -> DeviceInfo:
        host, port = self.coordinator.server_id.split(":", 1)
        return DeviceInfo(
            identifiers={(DOMAIN, f"server_{self.coordinator.entry_id}")},
            name=f"Sonium @ {host}:{port}",
            manufacturer="Sonium",
            model="Audio Server",
        )

    def client_device_info(self, client_id: str) -> DeviceInfo:
        client = self.coordinator.data.clients.get(client_id)
        name = client.name if client else client_id
        model = client.client_name if client else "Sonium Client"
        return DeviceInfo(
            identifiers={(DOMAIN, f"client_{client_id}")},
            name=name,
            manufacturer="Sonium",
            model=model,
            via_device=(DOMAIN, f"server_{self.coordinator.entry_id}"),
        )
