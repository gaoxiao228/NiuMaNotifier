import { createConnectionStateService, type ConnectionState } from '../modules/connections/connection-state.service.js'
import { createConnectionTokenService } from '../modules/connections/connection-token.service.js'
import { relayBindSchema, relayFrameSchema, type RelaySide } from '../modules/relay/relay.schemas.js'

export type RelayActor =
  | { kind: 'client'; userId: string }
  | { kind: 'device'; deviceId: string }

export type RelayBinding = {
  connectionId: string
  side: RelaySide
  userId: string
  deviceId: string
  clientId: string
}

export async function bindRelaySocket(input: {
  query: unknown
  actor: RelayActor
  tokenPepper: string
  state: { get(connectionId: string): Promise<ConnectionState | null> }
}): Promise<{ ok: true; binding: RelayBinding } | { ok: false; code: number; message: string }> {
  const parsed = relayBindSchema.safeParse(input.query)
  if (!parsed.success) return { ok: false, code: 100101, message: 'relay 连接参数无效' }

  const state = await input.state.get(parsed.data.connection_id)
  if (!state) return { ok: false, code: 220401, message: '连接不存在' }
  if (new Date(state.expires_at).getTime() <= Date.now()) {
    return { ok: false, code: 220402, message: '连接已过期' }
  }

  const tokenService = createConnectionTokenService({ tokenPepper: input.tokenPepper })
  if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  if (parsed.data.side === 'client' && (input.actor.kind !== 'client' || input.actor.userId !== state.user_id)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }
  if (parsed.data.side === 'device' && (input.actor.kind !== 'device' || input.actor.deviceId !== state.device_id)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  return {
    ok: true,
    binding: {
      connectionId: state.connection_id,
      side: parsed.data.side,
      userId: state.user_id,
      deviceId: state.device_id,
      clientId: state.client_id
    }
  }
}

export async function forwardRelayFrame(input: {
  raw: string
  binding: RelayBinding
  registry: {
    acceptSeq(connectionId: string, side: RelaySide, seq: number): boolean
    forward(connectionId: string, fromSide: RelaySide, message: object): boolean
  }
}): Promise<{ ok: true } | { ok: false; code: number; message: string }> {
  const frame = relayFrameSchema.parse(JSON.parse(input.raw))
  if (frame.connection_id !== input.binding.connectionId) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }
  if (!input.registry.acceptSeq(input.binding.connectionId, input.binding.side, frame.seq)) {
    return { ok: false, code: 220403, message: 'relay 帧序号无效' }
  }

  const sent = input.registry.forward(input.binding.connectionId, input.binding.side, frame)
  if (!sent) return { ok: false, code: 220404, message: '远程设备不可连接' }
  return { ok: true }
}

// Keeps imported state service visible to route registration added in the next task.
void createConnectionStateService
