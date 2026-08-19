from __future__ import annotations

import asyncio
import logging
from datetime import timedelta

from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.exceptions import ConfigEntryAuthFailed
from homeassistant.helpers.update_coordinator import DataUpdateCoordinator, UpdateFailed

from .api import CannotConnect, InvalidAuth, SoniumApiClient
from .const import CONF_HOST, CONF_PASSWORD, CONF_PORT, CONF_USERNAME
from .models import (
    HealthReport,
    SoniumClient,
    SoniumData,
    SoniumGroup,
    SoniumStream,
    StreamRecovery,
)

_LOGGER = logging.getLogger(__name__)
SCAN_INTERVAL = timedelta(seconds=60)


class SoniumCoordinator(DataUpdateCoordinator[SoniumData]):
    def __init__(
        self,
        hass: HomeAssistant,
        entry: ConfigEntry,
        api: SoniumApiClient,
    ) -> None:
        super().__init__(
            hass,
            _LOGGER,
            name="Sonium",
            config_entry=entry,
            update_interval=SCAN_INTERVAL,
        )
        self.api = api
        self._username: str = entry.data[CONF_USERNAME]
        self._password: str = entry.data[CONF_PASSWORD]
        self._ws_task: asyncio.Task | None = None
        self.server_id: str = f"{entry.data[CONF_HOST]}:{entry.data[CONF_PORT]}"
        self.entry_id: str = entry.entry_id

    async def _async_setup(self) -> None:
        try:
            await self.api.authenticate(self._username, self._password)
        except InvalidAuth as err:
            raise ConfigEntryAuthFailed("Invalid credentials") from err
        except CannotConnect as err:
            raise UpdateFailed(str(err)) from err

    async def _async_update_data(self) -> SoniumData:
        try:
            clients_raw, groups_raw, streams_raw = await asyncio.gather(
                self.api.get_clients(),
                self.api.get_groups(),
                self.api.get_streams(),
            )
        except InvalidAuth:
            try:
                await self.api.authenticate(self._username, self._password)
                clients_raw, groups_raw, streams_raw = await asyncio.gather(
                    self.api.get_clients(),
                    self.api.get_groups(),
                    self.api.get_streams(),
                )
            except (InvalidAuth, CannotConnect) as err:
                raise UpdateFailed(str(err)) from err
        except CannotConnect as err:
            raise UpdateFailed(str(err)) from err

        return SoniumData(
            clients={c["id"]: SoniumClient.from_dict(c) for c in clients_raw},
            groups={g["id"]: SoniumGroup.from_dict(g) for g in groups_raw},
            streams={s["id"]: SoniumStream.from_dict(s) for s in streams_raw},
        )

    async def async_start_websocket(self) -> None:
        if self._ws_task and not self._ws_task.done():
            return
        self._ws_task = self.hass.async_create_background_task(
            self._ws_loop(),
            name=f"sonium_ws_{self.server_id}",
        )

    async def _ws_loop(self) -> None:
        backoff = 5
        while True:
            try:
                _LOGGER.debug("WebSocket connecting to %s", self.server_id)
                async for event in self.api.subscribe_events():
                    backoff = 5
                    self._apply_event(event)
            except asyncio.CancelledError:
                return
            except Exception as err:  # noqa: BLE001
                _LOGGER.debug(
                    "WebSocket error (%s), reconnecting in %ss", err, backoff
                )
            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, 60)
            try:
                await self.api.authenticate(self._username, self._password)
            except Exception:  # noqa: BLE001
                pass

    @callback
    def _apply_event(self, event: dict) -> None:
        if self.data is None:
            return

        ev_type = event.get("type")
        data = self.data
        changed = True

        if ev_type == "volume_changed":
            c = data.clients.get(event["client_id"])
            if c:
                c.volume = event["volume"]
                c.muted = event["muted"]

        elif ev_type == "latency_changed":
            c = data.clients.get(event["client_id"])
            if c:
                c.latency_ms = event["latency_ms"]

        elif ev_type == "client_connected":
            raw = event.get("client", {})
            if raw.get("id"):
                data.clients[raw["id"]] = SoniumClient.from_dict(raw)

        elif ev_type == "client_disconnected":
            c = data.clients.get(event["client_id"])
            if c:
                c.status = "disconnected"

        elif ev_type == "client_deleted":
            data.clients.pop(event["client_id"], None)

        elif ev_type == "client_renamed":
            c = data.clients.get(event["client_id"])
            if c:
                c.display_name = event.get("display_name") or None

        elif ev_type == "client_group_changed":
            c = data.clients.get(event["client_id"])
            if c:
                old_gid = c.group_id
                new_gid = event["group_id"]
                c.group_id = new_gid
                old_g = data.groups.get(old_gid)
                if old_g and event["client_id"] in old_g.client_ids:
                    old_g.client_ids.remove(event["client_id"])
                new_g = data.groups.get(new_gid)
                if new_g and event["client_id"] not in new_g.client_ids:
                    new_g.client_ids.append(event["client_id"])

        elif ev_type == "group_created":
            raw = event.get("group", {})
            if raw.get("id"):
                data.groups[raw["id"]] = SoniumGroup.from_dict(raw)

        elif ev_type == "group_deleted":
            data.groups.pop(event["group_id"], None)

        elif ev_type == "group_renamed":
            g = data.groups.get(event["group_id"])
            if g:
                g.name = event["name"]

        elif ev_type == "group_stream_changed":
            g = data.groups.get(event["group_id"])
            if g:
                g.stream_id = event["stream_id"]

        elif ev_type == "stream_status":
            s = data.streams.get(event["stream_id"])
            if s:
                s.status = event["status"]
                s.recovery = (
                    StreamRecovery.from_dict(event["recovery"])
                    if event.get("recovery")
                    else None
                )

        elif ev_type == "client_health":
            c = data.clients.get(event["client_id"])
            if c and event.get("health"):
                c.health = HealthReport.from_dict(event["health"])

        elif ev_type in ("stream_level", "heartbeat", "transport_mode_changed",
                         "stream_eq_changed", "client_observability_changed"):
            changed = False

        else:
            _LOGGER.debug("Unhandled event type: %s", ev_type)
            changed = False

        if changed:
            self.async_set_updated_data(data)

    async def async_shutdown(self) -> None:
        if self._ws_task and not self._ws_task.done():
            self._ws_task.cancel()
            try:
                await self._ws_task
            except asyncio.CancelledError:
                pass
        await super().async_shutdown()
