# Distributed Audio Mixer — Codex handoff

## 1. Mission
Build a software-defined digital mixing console with bidirectional network audio. Combine the virtual audio routing capabilities of Voicemeeter with the channel processing and operational workflow of a digital mixing console. The primary use case is an independent church broadcast mix, with playback from media computers routed back to the physical console.

The product must be standalone. Future integration with Lumo is optional. Do not couple the core to Soundcraft hardware.

## 2. Target deployment
- Soundcraft Ui24R connected through bidirectional multichannel USB to a local audio server.
- The server captures individual console channels and produces an independent broadcast mix.
- The OBS computer receives the broadcast mix through a virtual capture device.
- Media computers send playback audio to the server through virtual playback devices. The server routes it to the broadcast mix, physical console USB returns, or both.
- Operators use a browser to control the server and monitor selected buses.
- The server can run on a dedicated mini-PC or on the OBS computer. A machine can run both server and client roles.
- The physical PA must continue to operate if the server fails. Server-dependent playback can be interrupted.

## 3. Components
### Audio Server
Own the audio graph, device management, DSP, clock synchronization, network streams, routing, meters and persistent configuration. Run without a graphical session where the platform permits it. Continue processing when control clients disconnect.

### Mixer UI
Provide a responsive web console with channel strips, faders, mute, pan, meters, input trim, channel names, EQ, gate, compressor, bus sends, master controls, PFL/AFL, routing matrix and scenes. Synchronize state across operators. Display device and stream health. Do not perform production mixing in the browser.

### Audio Bridge
Provide a native client for media and broadcast computers. Send and receive independent audio streams simultaneously. Support multiple named virtual playback and capture endpoints. Applications must be able to select endpoints using normal OS audio settings. Example: Lumo playback, browser playback, system audio and OBS capture. A bridge can also use physical audio devices and headphones.

## 4. Signal model
Source -> input channel -> trim/polarity -> gate -> parametric EQ -> compressor -> fader/pan -> bus sends -> bus DSP/limiter -> output endpoint.

Support mono and stereo sources, explicit channel mapping, pre/post-fader sends, independent bus send levels, and multiple output destinations. Distinguish a shared channel processor from optional bus-specific processing. Do not assume a routing matrix alone provides independent mixes. Support broadcast, PA return, recording and monitor buses. Add DCA groups after the core graph is stable.

Prevent accidental feedback: reject cyclic graph connections; warn about possible external USB/console loops; keep PA return routes disabled by default. Solo/PFL must affect only the requesting operator's monitor path, never the broadcast or PA output.

## 5. Functional requirements
| ID | Requirement | Acceptance |
|---|---|---|
| FR-01 | Enumerate audio devices and their channel capabilities. | Display available inputs, outputs, sample rates and device state. |
| FR-02 | Capture and render multichannel audio simultaneously. | Map distinct test signals to independent inputs and USB outputs without channel swaps. |
| FR-03 | Process channels in real time. | Verify gain, mute, pan, EQ, gate and compression with deterministic signal tests. |
| FR-04 | Provide independent buses and sends. | Changing a broadcast send does not change the PA return or monitor mix. |
| FR-05 | Route sources to multiple destinations. | A media stream reaches OBS and a configured USB return simultaneously. |
| FR-06 | Provide remote control. | Two browser clients observe consistent mixer state; disconnecting both does not stop audio. |
| FR-07 | Provide individual monitoring. | Two operators can listen to different buses or solo channels without changing program output. |
| FR-08 | Provide bidirectional native bridge transport. | A client sends playback and receives broadcast audio simultaneously. |
| FR-09 | Expose OS audio endpoints. | OBS can capture a named endpoint and a media application can play to another endpoint. |
| FR-10 | Persist mixer configuration and scenes. | Restart restores valid routing and settings; missing devices remain visibly disconnected. |
| FR-11 | Recover from device and network faults. | Disconnect/reconnect a bridge or USB device without crashing; recover with controlled fades. |
| FR-12 | Discover and authenticate clients. | Discover LAN server or enter address manually; reject unauthorized control and audio sessions. |

## 6. Technical direction (proposed, not mandated)
- Native audio engine: evaluate C++ with JUCE versus Rust with suitable audio/DSP libraries. Check licensing, deployment and multichannel support before selection.
- Server OS: validate Ui24R full-duplex USB support on Linux (ALSA/PipeWire) and Windows (vendor ASIO). Do not assume either platform exposes all channels without testing. Select the first supported platform based on evidence.
- Native audio APIs: ASIO/WASAPI on Windows; ALSA/PipeWire on Linux. Validate exclusive/shared access and sample rate behavior.
- Network audio: evaluate RTP/UDP with PCM for LAN production audio; define session negotiation, timestamps, sequence numbers, channel layout, jitter buffer and loss handling. WebRTC/Opus is a candidate for browser and WAN monitoring, not a mandatory production transport.
- Control plane: HTTP API plus WebSocket state updates; React/TypeScript web UI; mDNS discovery with manual fallback.
- Virtual endpoints: use existing virtual audio devices during the first end-to-end proof of concept. Evaluate native Windows virtual audio driver implementation, signing, installation, licensing and ASIO exposure separately. ASIO alone does not create general Windows playback endpoints.
- Persistence: versioned local configuration and scene storage; atomic writes and migrations.

## 7. Real-time and reliability constraints
- Use a real-time audio callback. Do not allocate memory, block, perform network/disk I/O, or acquire contended locks in the callback.
- Use bounded queues/ring buffers between audio, network and control threads. Apply graph changes safely at block boundaries. Smooth gain changes.
- Treat each independent hardware or network clock as a separate clock domain. Implement drift estimation and adaptive resampling where needed. Do not assume equal nominal sample rates imply synchronization.
- Start with 48 kHz and configurable block sizes; benchmark 64/128/256 samples. Use float32 internally. Negotiate endpoint formats explicitly.
- Initial engineering targets, subject to measurement: 24 mono inputs plus 4 stereo network inputs, at least 4 stereo buses, wired LAN one-way audio latency below 20 ms where practical, and a two-hour run without engine underruns under normal load. Report actual latency, CPU, jitter and dropouts; do not claim unmeasured targets are met.
- Keep monitoring latency distinct from production latency. WAN monitoring can be delayed and must not be part of the production signal path.
- Use authenticated control, role-based permissions (admin/operator/monitor), encrypted remote access and explicit pairing. Default to LAN-only. Do not expose unauthenticated audio/control ports to the internet.

## 8. Implementation plan
### Phase 0 — Feasibility and decisions
Inspect the repository. If empty, initialize a documented workspace. Research official device/API documentation and licenses. Produce ADRs for language, audio backend, transport and endpoint strategy. Build small probes for device enumeration, multichannel duplex I/O and network clock drift. Identify tests that require physical Ui24R access. Do not fabricate hardware validation.

### Phase 1 — Local engine
Implement graph, simulated devices, multichannel hardware I/O, channel DSP, buses, sends, routing, meters and configuration. Add offline DSP tests and a loopback test harness. Demonstrate independent mixes with generated signals.

### Phase 2 — Remote console
Implement control API, state synchronization and browser mixer. Add routing matrix, scenes, permissions and diagnostics. Verify that the engine survives browser disconnects.

### Phase 3 — Network bridge
Implement native bidirectional transport, jitter buffering, clock drift compensation, stream discovery and reconnection. Use physical or existing virtual endpoints for integration. Demonstrate media playback to USB return and broadcast output to OBS.

### Phase 4 — Monitoring and deployment
Add per-operator monitoring, PFL/AFL, browser listening, service installation, startup recovery, logs and performance diagnostics. Validate on the target church network with hardware available.

### Phase 5 — Product-owned virtual devices
Implement or integrate a legally distributable virtual endpoint solution. Support multiple named playback/capture devices, installation and removal, format negotiation and recovery. Add ASIO exposure if required by target applications. Treat this as a separate milestone, not a prerequisite for validating the mixer.

## 9. Suggested repository structure
```text
apps/server/
apps/bridge/
apps/mixer-web/
packages/audio-core/
packages/audio-backends/
packages/network-audio/
packages/control-protocol/
tests/
docs/adr/
docs/spec/
```
Adapt to the selected language and build system. Keep DSP independent of transport, UI and hardware adapters.

## 10. MVP end-to-end acceptance
1. Start the server with a physical or simulated multichannel device.
2. Open the mixer in two browsers and control individual channels.
3. Produce an independent stereo broadcast mix.
4. Receive that mix on a separate OBS computer as an audio input.
5. Send stereo playback from a media computer to the server.
6. Route playback independently to the broadcast mix and physical USB returns.
7. Monitor the broadcast and solo channels without affecting program audio.
8. Close the UI and confirm uninterrupted processing.
9. Restart the server and restore the scene safely.
10. Test network interruption, device removal, clock drift and sustained load. Record results and any hardware-dependent checks still pending.

## 11. Instructions to Codex
Act as the implementation owner. Read this document before modifying code. Maintain a living specification, ADRs, implementation checklist and test evidence in the repository. Use controlled, unambiguous technical language inspired by ASD-STE100; do not claim formal compliance without verification. Prefer small, testable vertical slices. Use subagents for independent research or review if available. Do not implement the entire system as a UI mockup. Prioritize the real audio path and measurable behavior. Do not install unsigned kernel drivers or change host security settings without explicit approval. Ask for decisions only when they materially affect scope, safety, licensing or irreversible architecture. Start with Phase 0, then proceed with the first validated vertical slice. Report blockers, assumptions, actual tests and remaining work.

## 12. Open decisions
Product name, supported OS matrix, final language/framework, driver strategy, transport protocol, maximum channel count, WAN access and deployment packaging remain open. The targets and technology candidates above are proposed engineering defaults, not previously approved commitments. Hardware testing requires access to the Ui24R and target computers.
