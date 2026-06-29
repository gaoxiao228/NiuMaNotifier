import type { RemoteTransport } from './remoteTransport.js'

export type WebRtcOfferSignal = {
  connection_id: string
  sdp: string
}

export type WebRtcAnswerSignal = {
  connection_id: string
  sdp: string
}

export type WebRtcIceCandidateSignal = {
  connection_id: string
  candidate: string
  sdp_mid?: string | null
  sdp_mline_index?: number | null
}

export type WebRtcTransportOptions = {
  connectionId: string
  iceServers?: RTCIceServer[]
  RTCPeerConnectionImpl?: typeof RTCPeerConnection
  onOffer(offer: WebRtcOfferSignal): void
  onIceCandidate(candidate: WebRtcIceCandidateSignal): void
  onOpen(): void
  onPayload(value: unknown): void
  onClose(): void
  onError(error: Error): void
}

export type WebRtcTransport = RemoteTransport & {
  start(): Promise<void>
  acceptAnswer(answer: WebRtcAnswerSignal): Promise<void>
  addRemoteIceCandidate(candidate: WebRtcIceCandidateSignal): Promise<void>
}

function toError(value: unknown, fallback: string): Error {
  return value instanceof Error ? value : new Error(fallback)
}

export function createWebRtcTransport(options: WebRtcTransportOptions): WebRtcTransport {
  const PeerConnection = options.RTCPeerConnectionImpl ?? RTCPeerConnection
  const peer = new PeerConnection({
    iceServers: options.iceServers ?? []
  })
  const channel = peer.createDataChannel('niuma-remote')
  let open = false
  let closed = false

  channel.onopen = () => {
    if (closed) return
    open = true
    options.onOpen()
  }
  channel.onmessage = (event) => {
    if (closed) return
    try {
      options.onPayload(JSON.parse(String(event.data)) as unknown)
    } catch (error) {
      options.onError(toError(error, 'WebRTC payload decode failed'))
    }
  }
  channel.onclose = () => {
    if (closed) return
    open = false
    options.onClose()
  }
  channel.onerror = () => {
    if (!closed) options.onError(new Error('WebRTC data channel error'))
  }

  peer.onicecandidate = (event) => {
    if (!event.candidate) return
    options.onIceCandidate({
      connection_id: options.connectionId,
      candidate: event.candidate.candidate,
      sdp_mid: event.candidate.sdpMid,
      sdp_mline_index: event.candidate.sdpMLineIndex
    })
  }
  peer.onconnectionstatechange = () => {
    if (closed) return
    if (peer.connectionState === 'failed') {
      options.onError(new Error('WebRTC connection failed'))
    }
    if (peer.connectionState === 'closed' || peer.connectionState === 'disconnected') {
      open = false
      options.onClose()
    }
  }

  return {
    kind: 'webrtc',
    async start() {
      const offer = await peer.createOffer()
      await peer.setLocalDescription(offer)
      options.onOffer({
        connection_id: options.connectionId,
        sdp: offer.sdp ?? ''
      })
    },
    async acceptAnswer(answer) {
      if (answer.connection_id !== options.connectionId) return
      await peer.setRemoteDescription({ type: 'answer', sdp: answer.sdp })
    },
    async addRemoteIceCandidate(candidate) {
      if (candidate.connection_id !== options.connectionId) return
      await peer.addIceCandidate({
        candidate: candidate.candidate,
        sdpMid: candidate.sdp_mid ?? null,
        sdpMLineIndex: candidate.sdp_mline_index ?? null
      })
    },
    send(value) {
      if (!open) throw new Error('WebRTC data channel is not open')
      channel.send(JSON.stringify(value))
    },
    close() {
      if (closed) return
      closed = true
      open = false
      channel.close()
      peer.close()
    }
  }
}
