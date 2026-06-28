import { z } from 'zod'

const baseMessageSchema = z.object({
  version: z.literal(1),
  id: z.string().min(1).max(160)
})

export const deviceHelloMessageSchema = baseMessageSchema.extend({
  type: z.literal('device.hello'),
  data: z.object({
    device_id: z.string().min(1).max(160),
    agent_protocol_version: z.number().int().positive(),
    rpc_protocol_version: z.number().int().positive(),
    capabilities: z.record(z.unknown())
  })
})

export const deviceHeartbeatMessageSchema = baseMessageSchema.extend({
  type: z.literal('device.heartbeat'),
  data: z.object({}).default({})
})

export const deviceSocketMessageSchema = z.discriminatedUnion('type', [
  deviceHelloMessageSchema,
  deviceHeartbeatMessageSchema
])

export type DeviceSocketMessage = z.infer<typeof deviceSocketMessageSchema>
