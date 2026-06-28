import type { HttpClient } from './httpClient.js'

export type RemoteDevice = {
  id: string
  name: string
  online: boolean
  last_seen_at: string | null
  capabilities: unknown
  identity_public_key: unknown
}

export type DeviceListResponse = {
  list: RemoteDevice[]
}

export function createDevicesApi(http: HttpClient) {
  return {
    list() {
      return http.get<DeviceListResponse>('/api/v1/devices/list')
    }
  }
}
