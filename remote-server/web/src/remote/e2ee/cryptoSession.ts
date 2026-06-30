import type { ClientHello, DeviceHello, RpcFrame, RpcRequest, RpcResponse } from './types.js'

export type ClientSession = {
  connectionId: string
  deviceId: string
  clientId: string
  sendKey: CryptoKey
  receiveKey: CryptoKey
  nextSeq: number
}

function encode(value: unknown): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(value))
}

function decode<T>(value: ArrayBuffer): T {
  return JSON.parse(new TextDecoder().decode(value)) as T
}

function base64url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
    .replaceAll('+', '-')
    .replaceAll('/', '_')
    .replaceAll('=', '')
}

function toBufferSource(bytes: Uint8Array): Uint8Array<ArrayBuffer> {
  // WebCrypto 的 DOM 类型要求 ArrayBuffer backing，避免 SharedArrayBuffer 类型混入。
  return new Uint8Array(Array.from(bytes))
}

function fromBase64url(value: string): Uint8Array<ArrayBuffer> {
  const padded = value.replaceAll('-', '+').replaceAll('_', '/') + '='.repeat((4 - (value.length % 4)) % 4)
  return new Uint8Array(Array.from(atob(padded), (char) => char.charCodeAt(0)))
}

async function importAesKey(bytes: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey('raw', toBufferSource(bytes), 'AES-GCM', false, ['encrypt', 'decrypt'])
}

async function deriveDirectionalKeys(sharedSecret: Uint8Array, context: string) {
  const material = await crypto.subtle.importKey('raw', toBufferSource(sharedSecret), 'HKDF', false, ['deriveBits'])
  const bits = await crypto.subtle.deriveBits(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: new TextEncoder().encode(`niuma-remote-e2ee-v1:${context}`),
      info: new TextEncoder().encode('client->device|device->client')
    },
    material,
    512
  )
  const bytes = new Uint8Array(bits)
  return {
    clientToDevice: await importAesKey(bytes.slice(0, 32)),
    deviceToClient: await importAesKey(bytes.slice(32, 64))
  }
}

export async function createClientHello(input: {
  connectionId: string
  deviceId: string
  clientId: string
}): Promise<ClientHello> {
  const keyPair = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits'])
  const publicKey = await crypto.subtle.exportKey('jwk', keyPair.publicKey)
  return {
    version: 1,
    type: 'e2ee.client_hello',
    connection_id: input.connectionId,
    device_id: input.deviceId,
    client_id: input.clientId,
    ephemeral_public_key: publicKey
  }
}

function deviceHelloSigningPayload(input: {
  connection_id: string
  device_id: string
  client_id: string
  ephemeral_public_key: JsonWebKey
}) {
  // 签名覆盖设备会话公钥和连接上下文，避免服务端伪造 device_hello。
  return encode({
    version: 1,
    type: 'e2ee.device_hello',
    connection_id: input.connection_id,
    device_id: input.device_id,
    client_id: input.client_id,
    ephemeral_public_key: input.ephemeral_public_key
  })
}

export async function signDeviceHelloForTest(input: {
  identitySigningKeyForTest: CryptoKey
  connectionId: string
  deviceId: string
  clientId: string
}): Promise<DeviceHello> {
  const ephemeral = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits'])
  const ephemeralPublicKey = await crypto.subtle.exportKey('jwk', ephemeral.publicKey)
  const payload = {
    connection_id: input.connectionId,
    device_id: input.deviceId,
    client_id: input.clientId,
    ephemeral_public_key: ephemeralPublicKey
  }
  const signature = await crypto.subtle.sign(
    { name: 'ECDSA', hash: 'SHA-256' },
    input.identitySigningKeyForTest,
    toBufferSource(deviceHelloSigningPayload(payload))
  )
  return {
    version: 1,
    type: 'e2ee.device_hello',
    ...payload,
    signature: base64url(new Uint8Array(signature))
  }
}

export async function verifyDeviceHello(
  deviceHello: Pick<DeviceHello, 'connection_id' | 'device_id' | 'client_id' | 'ephemeral_public_key' | 'signature'>,
  identityPublicKey: JsonWebKey
): Promise<boolean> {
  const key = await crypto.subtle.importKey(
    'jwk',
    identityPublicKey,
    { name: 'ECDSA', namedCurve: 'P-256' },
    false,
    ['verify']
  )
  return crypto.subtle.verify(
    { name: 'ECDSA', hash: 'SHA-256' },
    key,
    fromBase64url(deviceHello.signature),
    toBufferSource(deviceHelloSigningPayload(deviceHello))
  )
}

export async function deriveClientSession(input: {
  connectionId: string
  deviceId: string
  clientId: string
  sharedSecretForTest: Uint8Array
}): Promise<ClientSession> {
  const keys = await deriveDirectionalKeys(
    input.sharedSecretForTest,
    `${input.connectionId}:${input.deviceId}:${input.clientId}`
  )
  return {
    connectionId: input.connectionId,
    deviceId: input.deviceId,
    clientId: input.clientId,
    sendKey: keys.clientToDevice,
    receiveKey: keys.deviceToClient,
    nextSeq: 1
  }
}

export async function encryptFrame(session: ClientSession, payload: RpcRequest | RpcResponse): Promise<RpcFrame> {
  const nonce = crypto.getRandomValues(new Uint8Array(12))
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce },
    session.sendKey,
    toBufferSource(encode(payload))
  )
  return {
    version: 1,
    type: 'rpc.frame',
    connection_id: session.connectionId,
    seq: session.nextSeq++,
    nonce: base64url(nonce),
    ciphertext: base64url(new Uint8Array(ciphertext))
  }
}

export async function decryptFrameForTest(session: ClientSession, frame: RpcFrame): Promise<RpcRequest | RpcResponse> {
  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: fromBase64url(frame.nonce) },
    session.sendKey,
    fromBase64url(frame.ciphertext)
  )
  return decode<RpcRequest | RpcResponse>(plaintext)
}
