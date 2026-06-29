import { describe, expect, it, vi } from 'vitest'

import { createWebRtcTransport } from '../remote/webrtcTransport.js'

class FakeDataChannel {
  onopen: (() => void) | null = null
  onmessage: ((event: { data: string }) => void) | null = null
  onclose: (() => void) | null = null
  onerror: (() => void) | null = null
  sent: string[] = []
  closed = false

  send(value: string) {
    this.sent.push(value)
  }

  close() {
    this.closed = true
    this.onclose?.()
  }
}

class FakePeerConnection {
  static instances: FakePeerConnection[] = []

  onicecandidate: ((event: { candidate: null | { candidate: string; sdpMid: string | null; sdpMLineIndex: number | null } }) => void) | null = null
  onconnectionstatechange: (() => void) | null = null
  connectionState = 'new'
  dataChannel = new FakeDataChannel()
  localDescription: RTCSessionDescriptionInit | null = null
  remoteDescription: RTCSessionDescriptionInit | null = null
  addedCandidates: RTCIceCandidateInit[] = []
  closed = false

  constructor(readonly config: RTCConfiguration) {
    FakePeerConnection.instances.push(this)
  }

  createDataChannel(label: string) {
    expect(label).toBe('niuma-remote')
    return this.dataChannel as unknown as RTCDataChannel
  }

  async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: 'offer', sdp: 'offer-sdp' }
  }

  async setLocalDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = description
  }

  async setRemoteDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = description
  }

  async addIceCandidate(candidate: RTCIceCandidateInit): Promise<void> {
    this.addedCandidates.push(candidate)
  }

  close() {
    this.closed = true
  }
}

describe('createWebRtcTransport', () => {
  it('creates an offer and reports local ICE candidates', async () => {
    FakePeerConnection.instances = []
    const onOffer = vi.fn()
    const onIceCandidate = vi.fn()
    const transport = createWebRtcTransport({
      connectionId: 'conn_1',
      RTCPeerConnectionImpl: FakePeerConnection as unknown as typeof RTCPeerConnection,
      onOffer,
      onIceCandidate,
      onOpen: vi.fn(),
      onPayload: vi.fn(),
      onClose: vi.fn(),
      onError: vi.fn()
    })

    await transport.start()
    const peer = FakePeerConnection.instances[0]
    peer.onicecandidate?.({
      candidate: {
        candidate: 'candidate:1',
        sdpMid: '0',
        sdpMLineIndex: 0
      }
    })

    expect(onOffer).toHaveBeenCalledWith({ connection_id: 'conn_1', sdp: 'offer-sdp' })
    expect(onIceCandidate).toHaveBeenCalledWith({
      connection_id: 'conn_1',
      candidate: 'candidate:1',
      sdp_mid: '0',
      sdp_mline_index: 0
    })
  })

  it('sends and receives JSON payloads through the data channel', () => {
    FakePeerConnection.instances = []
    const onPayload = vi.fn()
    const onOpen = vi.fn()
    const transport = createWebRtcTransport({
      connectionId: 'conn_1',
      RTCPeerConnectionImpl: FakePeerConnection as unknown as typeof RTCPeerConnection,
      onOffer: vi.fn(),
      onIceCandidate: vi.fn(),
      onOpen,
      onPayload,
      onClose: vi.fn(),
      onError: vi.fn()
    })
    const peer = FakePeerConnection.instances[0]

    peer.dataChannel.onopen?.()
    transport.send({ version: 1, type: 'request', id: 'rpc_1' })
    peer.dataChannel.onmessage?.({ data: JSON.stringify({ version: 1, type: 'response', id: 'rpc_1' }) })

    expect(onOpen).toHaveBeenCalledTimes(1)
    expect(JSON.parse(peer.dataChannel.sent[0])).toEqual({ version: 1, type: 'request', id: 'rpc_1' })
    expect(onPayload).toHaveBeenCalledWith({ version: 1, type: 'response', id: 'rpc_1' })
  })

  it('applies remote answer and ICE candidates', async () => {
    FakePeerConnection.instances = []
    const transport = createWebRtcTransport({
      connectionId: 'conn_1',
      RTCPeerConnectionImpl: FakePeerConnection as unknown as typeof RTCPeerConnection,
      onOffer: vi.fn(),
      onIceCandidate: vi.fn(),
      onOpen: vi.fn(),
      onPayload: vi.fn(),
      onClose: vi.fn(),
      onError: vi.fn()
    })
    const peer = FakePeerConnection.instances[0]

    await transport.acceptAnswer({ connection_id: 'conn_1', sdp: 'answer-sdp' })
    await transport.addRemoteIceCandidate({
      connection_id: 'conn_1',
      candidate: 'candidate:remote',
      sdp_mid: '0',
      sdp_mline_index: 0
    })

    expect(peer.remoteDescription).toEqual({ type: 'answer', sdp: 'answer-sdp' })
    expect(peer.addedCandidates).toEqual([
      {
        candidate: 'candidate:remote',
        sdpMid: '0',
        sdpMLineIndex: 0
      }
    ])
  })
})
