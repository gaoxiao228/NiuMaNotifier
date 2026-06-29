# Remote Transport Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first implementation slice for direct-preferred remote transport by adding channel metadata, observed transport handling, and stream sequencing on the existing Relay path.

**Architecture:** Keep the current Relay data path working while introducing protocol fields that WebRTC will reuse later. Web payloads flow through a typed inbound message wrapper with `observedTransport`; device-side stream notifications include `transport.kind` and per-stream `seq`.

**Tech Stack:** TypeScript/Vitest for remote Web client, Rust/cargo tests for desktop remote RPC and Local API bridge.

---

### Task 1: Web RPC channel metadata and observed transport

**Files:**
- Modify: `remote-server/web/src/remote/plainRpcClient.ts`
- Modify: `remote-server/web/src/__tests__/plainRpcClient.test.ts`

- [ ] Step 1: Add failing tests that requests include `transport.kind = relay` and notifications expose `observedTransport`.
- [ ] Step 2: Run `npm test -- plainRpcClient.test.ts` and verify the new tests fail.
- [ ] Step 3: Add `RemoteTransportKind`, `RemoteTransportMetadata`, `ObservedPlainRpcMessage`, and support wrapping inbound payloads with observed transport.
- [ ] Step 4: Run `npm test -- plainRpcClient.test.ts` and verify it passes.

### Task 2: Web stream seq filtering

**Files:**
- Modify: `remote-server/web/src/remote/remoteLocalApiClient.ts`
- Modify: `remote-server/web/src/__tests__/remoteLocalApiClient.test.ts`

- [ ] Step 1: Add failing tests that older `seq` events are ignored and accepted events expose `observedTransport`.
- [ ] Step 2: Run `npm test -- remoteLocalApiClient.test.ts` and verify the tests fail.
- [ ] Step 3: Track `lastSeq` per stream and pass observed transport through event handlers.
- [ ] Step 4: Run `npm test -- remoteLocalApiClient.test.ts` and verify it passes.

### Task 3: Rust stream event metadata and seq

**Files:**
- Modify: `src-tauri/src/remote/local_api_bridge.rs`

- [ ] Step 1: Add failing Rust tests for `stream_event_notification` including `transport.kind = relay` and monotonically increasing `seq`.
- [ ] Step 2: Run `cargo test -p niuma-desktop remote::local_api_bridge` and verify the tests fail.
- [ ] Step 3: Introduce a stream notification builder/state that increments seq and writes transport metadata.
- [ ] Step 4: Run `cargo test -p niuma-desktop remote::local_api_bridge` and verify it passes.

### Task 4: Wire observed transport through current Relay UI path

**Files:**
- Modify: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Modify: `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`

- [ ] Step 1: Add failing test proving Relay payloads are handled with `observedTransport = relay` and session events with seq still render.
- [ ] Step 2: Run `npm test -- deviceConsolePage.test.tsx` and verify it fails.
- [ ] Step 3: Wrap `relayClient.onPayload` into observed inbound messages and keep existing rendering behavior.
- [ ] Step 4: Run `npm test -- deviceConsolePage.test.tsx` and verify it passes.

### Task 5: Full verification

**Files:**
- No new files.

- [ ] Step 1: Run `cargo fmt --check && cargo test -p niuma-desktop`.
- [ ] Step 2: Run `cd remote-server/web && npm test && npm run build`.
- [ ] Step 3: Review `git diff --stat` and summarize implemented slice.
