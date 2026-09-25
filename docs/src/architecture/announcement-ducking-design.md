# Announcements and ducking

Sonium treats announcements as a first-class playback intent, not as a
temporary replacement stream. The server schedules an announcement against
one or more groups, and each client applies a bounded duck envelope to its
current music program while the announcement is active.

## Contract

An announcement has an idempotency key, target groups, priority, payload/source,
duck attenuation in dB, attack/release durations, maximum duration, and a
resume policy. Priorities are `music < chime < announcement < emergency`.
Higher priority interrupts lower priority audio; equal priority is queued. A
cancel/expiry always restores the prior program exactly once.

The control plane owns admission, queueing and observable state. The media
plane carries announcement frames with a scheduled server timestamp and intent
metadata. Clients must acknowledge `scheduled`, `started`, `completed`,
`cancelled`, or `failed`; lack of acknowledgement expires the operation.

## Music Assistant compatibility

Home Assistant exposes an idempotent `sonium.play_announcement` service and
maps `media_player.play_media` calls with announcement metadata to the same
operation. The implementation does not depend on Sendspin or emulate its wire
format.

## Safety

Announcements cannot grow without bound, bypass authentication, or allocate an
unbounded audio queue. Every group has a queue depth and duration budget. The
server validates targets, source format and duration before scheduling.
