from __future__ import annotations

import logging

import voluptuous as vol
from homeassistant.config_entries import ConfigEntry
from homeassistant.const import Platform
from homeassistant.core import HomeAssistant, ServiceCall
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .api import SoniumApiClient
from .const import CONF_HOST, CONF_PASSWORD, CONF_PORT, CONF_SSL, CONF_USERNAME, DOMAIN
from .coordinator import SoniumCoordinator

_LOGGER = logging.getLogger(__name__)

PLATFORMS = [
    Platform.MEDIA_PLAYER,
    Platform.SENSOR,
    Platform.BINARY_SENSOR,
    Platform.SELECT,
    Platform.NUMBER,
]

SoniumConfigEntry = ConfigEntry  # ConfigEntry[SoniumCoordinator]


async def async_setup(hass: HomeAssistant, config: dict) -> bool:
    """Register domain-level services."""

    async def _get_coordinator(call: ServiceCall) -> SoniumCoordinator | None:
        entries = hass.config_entries.async_entries(DOMAIN)
        if not entries:
            return None
        return entries[0].runtime_data

    async def handle_rename_client(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator:
            await coordinator.api.rename_client(
                call.data["client_id"], call.data["display_name"]
            )
            await coordinator.async_request_refresh()

    async def handle_rename_group(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator:
            await coordinator.api.rename_group(
                call.data["group_id"], call.data["name"]
            )
            await coordinator.async_request_refresh()

    async def handle_create_group(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator:
            await coordinator.api.create_group(
                call.data["name"], call.data["stream_id"]
            )
            await coordinator.async_request_refresh()

    async def handle_delete_group(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator:
            await coordinator.api.delete_group(call.data["group_id"])
            await coordinator.async_request_refresh()

    hass.services.async_register(
        DOMAIN,
        "rename_client",
        handle_rename_client,
        schema=vol.Schema(
            {vol.Required("client_id"): str, vol.Required("display_name"): str}
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "rename_group",
        handle_rename_group,
        schema=vol.Schema(
            {vol.Required("group_id"): str, vol.Required("name"): str}
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "create_group",
        handle_create_group,
        schema=vol.Schema(
            {vol.Required("name"): str, vol.Required("stream_id"): str}
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "delete_group",
        handle_delete_group,
        schema=vol.Schema({vol.Required("group_id"): str}),
    )

    return True


async def async_setup_entry(hass: HomeAssistant, entry: SoniumConfigEntry) -> bool:
    session = async_get_clientsession(hass)
    api = SoniumApiClient(
        entry.data[CONF_HOST],
        entry.data[CONF_PORT],
        session,
        entry.data.get(CONF_SSL, False),
    )
    coordinator = SoniumCoordinator(hass, entry, api)
    await coordinator.async_config_entry_first_refresh()
    entry.runtime_data = coordinator

    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)

    # Start WebSocket after platforms are set up
    await coordinator.async_start_websocket()

    return True


async def async_unload_entry(hass: HomeAssistant, entry: SoniumConfigEntry) -> bool:
    unloaded = await hass.config_entries.async_unload_platforms(entry, PLATFORMS)
    if unloaded:
        coordinator: SoniumCoordinator = entry.runtime_data
        await coordinator.async_shutdown()
    return unloaded
