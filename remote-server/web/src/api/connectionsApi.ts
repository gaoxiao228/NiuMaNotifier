import type { HttpClient } from './httpClient.js'

export type ConnectionCreateResult = {
  connection_id: string
  connection_token: string
  expires_at: string
  expires_in: number
  signaling_url: string | null
  relay_url: string | null
}

export function createConnectionsApi(http: HttpClient) {
  return {
    create(deviceId: string, clientId: string) {
      return http.post<ConnectionCreateResult>('/api/v1/connections/create', {
        device_id: deviceId,
        client_id: clientId,
        transport_preference: 'relay_first'
      })
    }
  }
}
