"""Validation and wire-contract helpers for Sonium announcements.

This module intentionally has no Home Assistant dependency.  It is the single
boundary where the integration turns Home Assistant service/media metadata into
the bounded, versioned REST contract accepted by Sonium's control plane.
"""

from __future__ import annotations

from time import time
from typing import Any, Iterable
from urllib.parse import urlparse

ANNOUNCEMENT_VERSION = 1
MAX_IDEMPOTENCY_KEY_BYTES = 128
MAX_TARGET_GROUPS = 32
MAX_SOURCE_BYTES = 2_048
MAX_RAMP_MS = 5_000
MAX_DURATION_MS = 120_000
MAX_EXPIRY_AHEAD_MS = 24 * 60 * 60 * 1_000
DEFAULT_ATTENUATION_DB = -18.0
DEFAULT_ATTACK_MS = 25
DEFAULT_RELEASE_MS = 100
DEFAULT_MAX_DURATION_MS = 30_000
EXPIRY_GRACE_MS = 30_000
VALID_PRIORITIES = frozenset(("music", "chime", "announcement", "emergency"))


class AnnouncementValidationError(ValueError):
    """An invocation cannot be represented safely by the server contract."""


def _now_ms() -> int:
    return int(time() * 1_000)


def announcement_options_from_media_kwargs(kwargs: dict[str, Any]) -> dict[str, Any] | None:
    """Extract Sonium options from Home Assistant's optional media metadata.

    Home Assistant versions and callers may carry custom media options either
    directly, in ``extra``, or in ``metadata``.  A normal ``play_media`` call
    is deliberately not treated as an announcement.
    """

    options: dict[str, Any] = {}
    for key in ("metadata", "extra"):
        value = kwargs.get(key)
        if isinstance(value, dict):
            options.update(value)
    options.update({key: value for key, value in kwargs.items() if key not in {"metadata", "extra"}})
    if options.get("announce") is not True:
        return None

    duck = options.get("duck")
    if not isinstance(duck, dict):
        duck = {}
    return {
        "priority": options.get("priority", "announcement"),
        "attenuation_db": duck.get("attenuation_db", options.get("attenuation_db", DEFAULT_ATTENUATION_DB)),
        "attack_ms": duck.get("attack_ms", options.get("attack_ms", DEFAULT_ATTACK_MS)),
        "release_ms": duck.get("release_ms", options.get("release_ms", DEFAULT_RELEASE_MS)),
        "max_duration_ms": options.get("max_duration_ms", DEFAULT_MAX_DURATION_MS),
        "resume": options.get("resume", True),
        "idempotency_key": options.get("idempotency_key"),
    }


def _validate_source(source: str) -> str:
    if not isinstance(source, str) or not source or len(source.encode()) > MAX_SOURCE_BYTES:
        raise AnnouncementValidationError("source must be a non-empty bounded URI")
    if any(char.isspace() or ord(char) < 32 for char in source):
        raise AnnouncementValidationError("source URI cannot contain whitespace or control characters")
    parsed = urlparse(source)
    if parsed.scheme not in {"http", "https", "media"} or not parsed.netloc:
        raise AnnouncementValidationError("source must be an http(s) or media URI")
    return source


def _validate_groups(target_groups: Iterable[str]) -> list[str]:
    groups = list(target_groups)
    if not groups:
        raise AnnouncementValidationError("at least one target group is required")
    if len(groups) > MAX_TARGET_GROUPS:
        raise AnnouncementValidationError(f"at most {MAX_TARGET_GROUPS} target groups are allowed")
    if any(not isinstance(group, str) or not group for group in groups):
        raise AnnouncementValidationError("target groups must be non-empty strings")
    if len(set(groups)) != len(groups):
        raise AnnouncementValidationError("target groups must be unique")
    return groups


def build_announcement_intent(
    *,
    source: str,
    target_groups: Iterable[str],
    idempotency_key: str,
    priority: str = "announcement",
    attenuation_db: float = DEFAULT_ATTENUATION_DB,
    attack_ms: int = DEFAULT_ATTACK_MS,
    release_ms: int = DEFAULT_RELEASE_MS,
    max_duration_ms: int = DEFAULT_MAX_DURATION_MS,
    resume: bool = True,
    now_ms: int | None = None,
) -> dict[str, Any]:
    """Build a locally validated announcement intent for ``POST /api/announcements``."""

    if not isinstance(idempotency_key, str) or not idempotency_key or len(idempotency_key.encode()) > MAX_IDEMPOTENCY_KEY_BYTES:
        raise AnnouncementValidationError("idempotency_key must contain 1..=128 bytes")
    if priority not in VALID_PRIORITIES:
        raise AnnouncementValidationError("priority must be music, chime, announcement, or emergency")
    if (
        isinstance(attenuation_db, bool)
        or not isinstance(attenuation_db, (int, float))
        or not -60 <= attenuation_db <= 0
    ):
        raise AnnouncementValidationError("duck attenuation must be between -60 and 0 dB")
    if isinstance(attack_ms, bool) or not isinstance(attack_ms, int) or not 0 <= attack_ms <= MAX_RAMP_MS:
        raise AnnouncementValidationError("duck attack_ms must be between 0 and 5000")
    if isinstance(release_ms, bool) or not isinstance(release_ms, int) or not 0 <= release_ms <= MAX_RAMP_MS:
        raise AnnouncementValidationError("duck release_ms must be between 0 and 5000")
    if isinstance(max_duration_ms, bool) or not isinstance(max_duration_ms, int) or not 1 <= max_duration_ms <= MAX_DURATION_MS:
        raise AnnouncementValidationError("max_duration_ms must be between 1 and 120000")
    if not isinstance(resume, bool):
        raise AnnouncementValidationError("resume must be a boolean")

    now = _now_ms() if now_ms is None else now_ms
    expires_at_ms = now + max_duration_ms + EXPIRY_GRACE_MS
    if expires_at_ms > now + MAX_EXPIRY_AHEAD_MS:
        raise AnnouncementValidationError("announcement expiry is too far in the future")

    return {
        "version": ANNOUNCEMENT_VERSION,
        "idempotency_key": idempotency_key,
        "target_groups": _validate_groups(target_groups),
        "priority": priority,
        "source": {"kind": "uri", "uri": _validate_source(source)},
        "duck": {
            "attenuation_db": float(attenuation_db),
            "attack_ms": attack_ms,
            "release_ms": release_ms,
        },
        "max_duration_ms": max_duration_ms,
        "expires_at_ms": expires_at_ms,
        "resume": "resume_previous" if resume else "do_not_resume",
    }
