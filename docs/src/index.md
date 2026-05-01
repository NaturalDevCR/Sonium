---
layout: home

hero:
  name: Sonium
  text: Local-first multiroom audio
  tagline: Run one server, connect lightweight clients, and keep speakers synchronized across your home network without cloud accounts or vendor lock-in.
  actions:
    - theme: brand
      text: Quick Start
      link: /getting-started/quick-start
    - theme: alt
      text: Install
      link: /getting-started/installation
    - theme: alt
      text: Configuration
      link: /getting-started/configuration

features:
  - title: Experimental, not production-ready
    details: Sonium is usable for testing, but still has known audio stability, sync, and operational gaps.
  - title: Server and client binaries
    details: The server ingests PCM audio and hosts the web UI. Clients discover or connect to the server and play synchronized audio.
  - title: Works on your LAN
    details: mDNS discovery, reconnects, per-client latency, volume, EQ, and groups are built into the control plane.
  - title: Built for operators
    details: Docker, systemd, GitHub releases, Prometheus metrics, config editing, log filters, and supervised restart requests are first-class workflows.
---

::: danger Not production-ready
Sonium is an early project. It can drop audio, misbehave under low buffers, and
change between releases. Do not use it for production, venues, alarms, or
unattended audio systems yet.
:::

## The Mental Model

Sonium has two moving parts:

| Part | Runs on | What it does |
| --- | --- | --- |
| `sonium-server` | A machine that receives audio | Reads a source, encodes stream chunks, serves the web UI/API, coordinates groups and clients. |
| `sonium-client` | Every playback device | Connects to the server, keeps time in sync, decodes audio, and writes to a local speaker. |

Most installs start with a server on a Linux box or Raspberry Pi, then a client on each speaker machine.
