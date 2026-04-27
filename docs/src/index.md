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
  - title: Server and client binaries
    details: The server ingests PCM audio and hosts the web UI. Clients discover or connect to the server and play synchronized audio.
  - title: Works on your LAN
    details: mDNS discovery, reconnects, per-client latency, volume, EQ, and groups are built into the control plane.
  - title: Built for operators
    details: Docker, systemd, GitHub releases, Prometheus metrics, config editing, and hot reload are first-class workflows.
---

## The Mental Model

Sonium has two moving parts:

| Part | Runs on | What it does |
| --- | --- | --- |
| `sonium-server` | A machine that receives audio | Reads a source, encodes stream chunks, serves the web UI/API, coordinates groups and clients. |
| `sonium-client` | Every playback device | Connects to the server, keeps time in sync, decodes audio, and writes to a local speaker. |

Most installs start with a server on a Linux box or Raspberry Pi, then a client on each speaker machine.
