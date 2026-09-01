from __future__ import annotations

import logging

import voluptuous as vol
from homeassistant.config_entries import ConfigEntry
from homeassistant.const import Platform
from homeassistant.core import HomeAssistant, ServiceCall
from homeassistant.exceptions import HomeAssistantError
from homeassistant.helpers import entity_registry as er
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .announcement import AnnouncementValidationError, build_announcement_intent
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

    def _resolve_announcement_groups(
        coordinator: SoniumCoordinator, call: ServiceCall
    ) -> list[str]:
        """Resolve explicit group IDs and Sonium entity IDs into unique zones."""
        groups = list(call.data.get("group_ids", []))
        registry = er.async_get(hass)
        group_prefix = f"{coordinator.entry_id}_group_"
        client_prefix = f"{coordinator.entry_id}_client_"
        for entity_id in call.data.get("target_entity_ids", []):
            entry = registry.async_get(entity_id)
            if entry is None or entry.platform != DOMAIN:
                raise HomeAssistantError(
                    f"{entity_id} is not a Sonium media player for this integration"
                )
            if entry.unique_id.startswith(group_prefix):
                groups.append(entry.unique_id[len(group_prefix) :])
            elif entry.unique_id.startswith(client_prefix):
                client_id = entry.unique_id[len(client_prefix) :]
                client = coordinator.data.clients.get(client_id)
                if client is None:
                    raise HomeAssistantError(f"Sonium client {client_id} is unavailable")
                groups.append(client.group_id)
            else:
                raise HomeAssistantError(
                    f"{entity_id} does not belong to this Sonium config entry"
                )

        if not groups:
            raise HomeAssistantError("Specify group_ids or target_entity_ids")
        if len(set(groups)) != len(groups):
            raise HomeAssistantError("Announcement targets resolve to duplicate groups")
        unknown = [group_id for group_id in groups if group_id not in coordinator.data.groups]
        if unknown:
            raise HomeAssistantError(f"Unknown Sonium group(s): {', '.join(unknown)}")
        return groups

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

    async def handle_play_announcement(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator is None:
            raise HomeAssistantError("No configured Sonium server is available")
        try:
            intent = build_announcement_intent(
                source=call.data["source"],
                target_groups=_resolve_announcement_groups(coordinator, call),
                idempotency_key=call.data["idempotency_key"],
                priority=call.data["priority"],
                attenuation_db=call.data["attenuation_db"],
                attack_ms=call.data["attack_ms"],
                release_ms=call.data["release_ms"],
                max_duration_ms=call.data["max_duration_ms"],
                resume=call.data["resume"],
            )
        except AnnouncementValidationError as err:
            raise HomeAssistantError(str(err)) from err
        await coordinator.api.create_announcement(intent)

    async def handle_cancel_announcement(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call)
        if coordinator is None:
            raise HomeAssistantError("No configured Sonium server is available")
        await coordinator.api.cancel_announcement(call.data["announcement_id"])

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
    hass.services.async_register(
        DOMAIN,
        "play_announcement",
        handle_play_announcement,
        schema=vol.Schema(
            {
                vol.Required("source"): vol.All(str, vol.Length(min=1, max=2048)),
                vol.Required("idempotency_key"): vol.All(str, vol.Length(min=1, max=128)),
                vol.Optional("group_ids", default=list): vol.All([str], vol.Length(max=32)),
                vol.Optional("target_entity_ids", default=list): vol.All([str], vol.Length(max=32)),
                vol.Optional("priority", default="announcement"): vol.In(
                    ("music", "chime", "announcement", "emergency")
                ),
                vol.Optional("attenuation_db", default=-18.0): vol.All(
                    vol.Coerce(float), vol.Range(min=-60, max=0)
                ),
                vol.Optional("attack_ms", default=25): vol.All(
                    vol.Coerce(int), vol.Range(min=0, max=5000)
                ),
                vol.Optional("release_ms", default=100): vol.All(
                    vol.Coerce(int), vol.Range(min=0, max=5000)
                ),
                vol.Optional("max_duration_ms", default=30000): vol.All(
                    vol.Coerce(int), vol.Range(min=1, max=120000)
                ),
                vol.Optional("resume", default=True): bool,
            }
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "cancel_announcement",
        handle_cancel_announcement,
        schema=vol.Schema(
            {vol.Required("announcement_id"): vol.All(str, vol.Length(min=1, max=128))}
        ),
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
