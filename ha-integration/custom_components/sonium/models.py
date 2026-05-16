from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class HealthReport:
    underrun_count: int = 0
    overrun_count: int = 0
    jitter_ms: float = 0.0
    buffer_depth_ms: float = 0.0
    latency_ms: float = 0.0

    @classmethod
    def from_dict(cls, d: dict) -> HealthReport:
        return cls(
            underrun_count=d.get("underrun_count", 0),
            overrun_count=d.get("overrun_count", 0),
            jitter_ms=d.get("jitter_ms", 0.0),
            buffer_depth_ms=d.get("buffer_depth_ms", 0.0),
            latency_ms=d.get("latency_ms", 0.0),
        )


@dataclass
class SoniumClient:
    id: str
    hostname: str
    display_name: Optional[str]
    volume: int
    muted: bool
    latency_ms: int
    group_id: str
    status: str
    client_name: str
    os: str
    arch: str
    remote_addr: str
    health: Optional[HealthReport] = None

    @property
    def name(self) -> str:
        return self.display_name or self.hostname

    @classmethod
    def from_dict(cls, d: dict) -> SoniumClient:
        health = None
        if d.get("health"):
            health = HealthReport.from_dict(d["health"])
        return cls(
            id=d["id"],
            hostname=d["hostname"],
            display_name=d.get("display_name") or None,
            volume=d.get("volume", 100),
            muted=d.get("muted", False),
            latency_ms=d.get("latency_ms", 0),
            group_id=d.get("group_id", "default"),
            status=d.get("status", "disconnected"),
            client_name=d.get("client_name", "Sonium Client"),
            os=d.get("os", ""),
            arch=d.get("arch", ""),
            remote_addr=d.get("remote_addr", ""),
            health=health,
        )


@dataclass
class SoniumGroup:
    id: str
    name: str
    stream_id: str
    client_ids: list[str] = field(default_factory=list)

    @classmethod
    def from_dict(cls, d: dict) -> SoniumGroup:
        return cls(
            id=d["id"],
            name=d["name"],
            stream_id=d.get("stream_id", ""),
            client_ids=list(d.get("client_ids", [])),
        )


@dataclass
class SoniumStream:
    id: str
    display_name: Optional[str]
    codec: str
    format: str
    source: str
    status: str

    @property
    def name(self) -> str:
        return self.display_name or self.id

    @classmethod
    def from_dict(cls, d: dict) -> SoniumStream:
        return cls(
            id=d["id"],
            display_name=d.get("display_name") or None,
            codec=d.get("codec", ""),
            format=d.get("format", ""),
            source=d.get("source", ""),
            status=d.get("status", "idle"),
        )


@dataclass
class SoniumData:
    clients: dict[str, SoniumClient] = field(default_factory=dict)
    groups: dict[str, SoniumGroup] = field(default_factory=dict)
    streams: dict[str, SoniumStream] = field(default_factory=dict)
