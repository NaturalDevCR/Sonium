from __future__ import annotations

import logging

import voluptuous as vol
from homeassistant.config_entries import ConfigEntry
from homeassistant.const import Platform
from homeassistant.core import HomeAssistant, ServiceCall, SupportsResponse
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

    def _loaded_coordinators() -> list[SoniumCoordinator]:
        """Return coordinators for loaded entries only.

        A Home Assistant instance may contain more than one Sonium server.  A
        service must never silently send a mutating request to the first entry
        in that case: entity targets and ``config_entry_id`` are the explicit
        routing keys, while an unambiguous resource ID remains a convenient
        backwards-compatible fallback.
        """
        coordinators: list[SoniumCoordinator] = []
        for entry in hass.config_entries.async_entries(DOMAIN):
            coordinator = getattr(entry, "runtime_data", None)
            if isinstance(coordinator, SoniumCoordinator):
                coordinators.append(coordinator)
        return coordinators

    def _target_entity_ids(call: ServiceCall) -> list[str]:
        """Combine the integration field and HA's standard entity target."""
        entity_ids: list[str] = []
        for key in ("target_entity_ids", "entity_id"):
            value = call.data.get(key, [])
            if isinstance(value, str):
                value = [value]
            if isinstance(value, (list, tuple, set)):
                entity_ids.extend(entity_id for entity_id in value if isinstance(entity_id, str))
        return list(dict.fromkeys(entity_ids))

    def _coordinator_for_entity_targets(
        coordinators: list[SoniumCoordinator], call: ServiceCall
    ) -> SoniumCoordinator | None:
        """Resolve entity targets to their owning config entry."""
        entity_ids = _target_entity_ids(call)
        if not entity_ids:
            return None
        registry = er.async_get(hass)
        entry_ids: set[str] = set()
        for entity_id in entity_ids:
            registry_entry = registry.async_get(entity_id)
            if registry_entry is None or registry_entry.platform != DOMAIN:
                raise HomeAssistantError(
                    f"{entity_id} is not a Sonium media player for this integration"
                )
            entry_id = getattr(registry_entry, "config_entry_id", None)
            if not isinstance(entry_id, str) or not entry_id:
                raise HomeAssistantError(f"{entity_id} has no Sonium config entry")
            entry_ids.add(entry_id)
        if len(entry_ids) != 1:
            raise HomeAssistantError(
                "Sonium targets belong to multiple config entries; split the call"
            )
        coordinator = next(
            (item for item in coordinators if item.entry_id in entry_ids), None
        )
        if coordinator is None:
            raise HomeAssistantError("The targeted Sonium config entry is not loaded")
        return coordinator

    async def _get_coordinator(
        call: ServiceCall,
        *,
        resource_keys: tuple[str, ...] = (),
        allow_entity_targets: bool = False,
    ) -> SoniumCoordinator | None:
        coordinators = _loaded_coordinators()
        if not coordinators:
            return None
        config_entry_id = call.data.get("config_entry_id")
        if config_entry_id is not None:
            coordinator = next(
                (item for item in coordinators if item.entry_id == config_entry_id), None
            )
            if coordinator is None:
                raise HomeAssistantError(
                    f"Sonium config entry {config_entry_id!r} is not loaded"
                )
            return coordinator

        if allow_entity_targets:
            targeted = _coordinator_for_entity_targets(coordinators, call)
            if targeted is not None:
                return targeted

        candidates = coordinators
        if resource_keys:
            candidates = [
                coordinator
                for coordinator in coordinators
                if all(
                    any(
                        key in collection
                        for collection in (
                            coordinator.data.clients,
                            coordinator.data.groups,
                            coordinator.data.streams,
                        )
                    )
                    for key in resource_keys
                )
            ]
        if len(candidates) == 1:
            return candidates[0]
        if len(candidates) > 1:
            raise HomeAssistantError(
                "Sonium service call is ambiguous across config entries; "
                "provide config_entry_id or an entity target"
            )
        if resource_keys:
            raise HomeAssistantError("The requested Sonium resource was not found")
        raise HomeAssistantError(
            "Multiple Sonium config entries are loaded; provide config_entry_id"
        )

    def _resolve_announcement_groups(
        coordinator: SoniumCoordinator, call: ServiceCall
    ) -> list[str]:
        """Resolve explicit group IDs and Sonium entity IDs into unique zones."""
        groups = list(call.data.get("group_ids", []))
        registry = er.async_get(hass)
        group_prefix = f"{coordinator.entry_id}_group_"
        client_prefix = f"{coordinator.entry_id}_client_"
        for entity_id in _target_entity_ids(call):
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
        coordinator = await _get_coordinator(
            call, resource_keys=(call.data["client_id"],)
        )
        if coordinator:
            await coordinator.api.rename_client(
                call.data["client_id"], call.data["display_name"]
            )
            await coordinator.async_request_refresh()

    async def handle_rename_group(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call, resource_keys=(call.data["group_id"],))
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
        coordinator = await _get_coordinator(call, resource_keys=(call.data["group_id"],))
        if coordinator:
            await coordinator.api.delete_group(call.data["group_id"])
            await coordinator.async_request_refresh()

    async def handle_play_announcement(call: ServiceCall) -> None:
        coordinator = await _get_coordinator(call, allow_entity_targets=True)
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
        admission = await coordinator.api.create_announcement(intent)
        announcement_id = admission.get("id")
        lifecycle = admission.get("lifecycle")
        if not isinstance(announcement_id, str) or not isinstance(lifecycle, str):
            raise HomeAssistantError("Sonium returned an invalid announcement admission")
        return {
            "announcement_id": announcement_id,
            "lifecycle": lifecycle,
            "duplicate": bool(admission.get("duplicate", False)),
        }

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
            {
                vol.Required("client_id"): str,
                vol.Required("display_name"): str,
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "rename_group",
        handle_rename_group,
        schema=vol.Schema(
            {
                vol.Required("group_id"): str,
                vol.Required("name"): str,
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "create_group",
        handle_create_group,
        schema=vol.Schema(
            {
                vol.Required("name"): str,
                vol.Required("stream_id"): str,
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
        ),
    )
    hass.services.async_register(
        DOMAIN,
        "delete_group",
        handle_delete_group,
        schema=vol.Schema(
            {
                vol.Required("group_id"): str,
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
        ),
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
                vol.Optional("entity_id"): vol.Any(str, [str]),
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
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
        ),
        supports_response=SupportsResponse.OPTIONAL,
    )
    hass.services.async_register(
        DOMAIN,
        "cancel_announcement",
        handle_cancel_announcement,
        schema=vol.Schema(
            {
                vol.Required("announcement_id"): vol.All(str, vol.Length(min=1, max=128)),
                vol.Optional("config_entry_id"): vol.All(str, vol.Length(min=1, max=128)),
            }
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
