from __future__ import annotations

import json
import logging
from typing import Any, AsyncGenerator

import aiohttp

_LOGGER = logging.getLogger(__name__)


class CannotConnect(Exception):
    pass


class InvalidAuth(Exception):
    pass


class SoniumApiClient:
    def __init__(
        self,
        host: str,
        port: int,
        session: aiohttp.ClientSession,
        ssl: bool = False,
    ) -> None:
        scheme = "https" if ssl else "http"
        self._host = host
        self._port = port
        self._ssl = ssl
        self._session = session
        self._token: str | None = None
        self._base_url = f"{scheme}://{host}:{port}"
        self._ws_scheme = "wss" if ssl else "ws"

    async def authenticate(self, username: str, password: str) -> None:
        try:
            async with self._session.post(
                f"{self._base_url}/api/auth/login",
                json={"username": username, "password": password},
                timeout=aiohttp.ClientTimeout(total=10),
                ssl=self._ssl,
            ) as resp:
                if resp.status == 401:
                    raise InvalidAuth("Invalid credentials")
                resp.raise_for_status()
                data = await resp.json()
                self._token = data["token"]
        except aiohttp.ClientConnectionError as err:
            raise CannotConnect(str(err)) from err

    def _headers(self) -> dict[str, str]:
        if self._token:
            return {"Authorization": f"Bearer {self._token}"}
        return {}

    async def _request(self, method: str, path: str, **kwargs: Any) -> Any:
        try:
            async with self._session.request(
                method,
                f"{self._base_url}{path}",
                headers=self._headers(),
                timeout=aiohttp.ClientTimeout(total=10),
                ssl=self._ssl,
                **kwargs,
            ) as resp:
                if resp.status == 401:
                    raise InvalidAuth("Token expired or invalid")
                resp.raise_for_status()
                if resp.content_type == "application/json":
                    return await resp.json()
                return None
        except aiohttp.ClientConnectionError as err:
            raise CannotConnect(str(err)) from err

    async def get_status(self) -> dict:
        return await self._request("GET", "/api/status")

    async def get_clients(self) -> list[dict]:
        return await self._request("GET", "/api/clients")

    async def get_groups(self) -> list[dict]:
        return await self._request("GET", "/api/groups")

    async def get_streams(self) -> list[dict]:
        return await self._request("GET", "/api/streams")

    async def set_volume(self, client_id: str, volume: int, muted: bool) -> None:
        await self._request(
            "PATCH",
            f"/api/clients/{client_id}/volume",
            json={"volume": volume, "muted": muted},
        )

    async def set_latency(self, client_id: str, latency_ms: int) -> None:
        await self._request(
            "PATCH",
            f"/api/clients/{client_id}/latency",
            json={"latency_ms": latency_ms},
        )

    async def set_client_group(self, client_id: str, group_id: str) -> None:
        await self._request(
            "PATCH",
            f"/api/clients/{client_id}/group",
            json={"group_id": group_id},
        )

    async def rename_client(self, client_id: str, display_name: str) -> None:
        await self._request(
            "PATCH",
            f"/api/clients/{client_id}/name",
            json={"display_name": display_name},
        )

    async def set_group_stream(self, group_id: str, stream_id: str) -> None:
        await self._request(
            "PATCH",
            f"/api/groups/{group_id}/stream",
            json={"stream_id": stream_id},
        )

    async def rename_group(self, group_id: str, name: str) -> None:
        await self._request(
            "PATCH",
            f"/api/groups/{group_id}",
            json={"name": name},
        )

    async def create_group(self, name: str, stream_id: str) -> dict:
        return await self._request(
            "POST",
            "/api/groups",
            json={"name": name, "stream_id": stream_id},
        )

    async def delete_group(self, group_id: str) -> None:
        await self._request("DELETE", f"/api/groups/{group_id}")

    async def create_announcement(self, intent: dict[str, Any]) -> dict[str, Any]:
        """Submit a locally validated, idempotent announcement intent."""
        response = await self._request("POST", "/api/announcements", json=intent)
        if not isinstance(response, dict):
            raise RuntimeError("Sonium returned an invalid announcement response")
        return response

    async def cancel_announcement(self, announcement_id: str) -> None:
        await self._request("DELETE", f"/api/announcements/{announcement_id}")

    async def subscribe_events(self) -> AsyncGenerator[dict, None]:
        ws_url = (
            f"{self._ws_scheme}://{self._host}:{self._port}"
            "/api/events"
        )
        ticket_response = await self._request("POST", "/api/events/ticket", json={})
        ticket = ticket_response.get("ticket") if isinstance(ticket_response, dict) else None
        if not isinstance(ticket, str) or not ticket:
            raise InvalidAuth("WebSocket ticket was not issued")
        async with self._session.ws_connect(
            ws_url,
            protocols=[ticket],
            heartbeat=30,
            ssl=self._ssl,
        ) as ws:
            async for msg in ws:
                if msg.type == aiohttp.WSMsgType.TEXT:
                    try:
                        event = json.loads(msg.data)
                        yield event
                    except json.JSONDecodeError:
                        _LOGGER.debug("Failed to parse WS message: %s", msg.data)
                elif msg.type in (aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.ERROR):
                    break
