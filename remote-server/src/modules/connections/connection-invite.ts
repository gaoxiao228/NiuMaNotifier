function mapInviteTransportPreference(input: 'webrtc_first' | 'relay_first' | 'relay_only') {
  // 本机端使用 auto/webrtc/relay；relay_first 和 relay_only 在当前 MVP 都走 relay。
  if (input === 'webrtc_first') return 'auto'
  return 'relay'
}

export function createConnectionInviteMessage(input: {
  connectionId: string
  connectionToken: string
  clientId: string
  transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
  expiresAt: string
}) {
  return {
    version: 1,
    type: 'connection.invite',
    id: `msg_${input.connectionId}`,
    data: {
      connection_id: input.connectionId,
      connection_token: input.connectionToken,
      client_id: input.clientId,
      transport_preference: mapInviteTransportPreference(input.transportPreference),
      expires_at: input.expiresAt
    }
  }
}
