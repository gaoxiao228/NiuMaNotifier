export type ClientHello = {
  version: 1
  type: 'e2ee.client_hello'
  connection_id: string
  device_id: string
  client_id: string
  ephemeral_public_key: JsonWebKey
}

export type DeviceHello = {
  version: 1
  type: 'e2ee.device_hello'
  connection_id: string
  device_id: string
  client_id: string
  ephemeral_public_key: JsonWebKey
  signature: string
}

export type RpcRequest = {
  version: 1
  type: 'request'
  id: string
  method: string
  params: Record<string, unknown>
}

export type RpcResponse = {
  version: 1
  type: 'response'
  id: string
  ok: boolean
  result?: unknown
  error?: { code: string; message: string }
}

export type RpcFrame = {
  version: 1
  type: 'rpc.frame'
  connection_id: string
  seq: number
  nonce: string
  ciphertext: string
}
