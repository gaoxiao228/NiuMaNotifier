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

const deviceConnectionResponseDataSchema = z.object({
  connection_id: z.string().min(1).max(160)
})

const sdpSchema = z.string().min(1).max(262144)
const reasonSchema = z.string().min(1).max(240)
const candidateSchema = z.string().min(1).max(8192)

export const deviceResponseMessageSchema = z.discriminatedUnion('type', [
  baseMessageSchema.extend({
    type: z.literal('connection.accept'),
    data: deviceConnectionResponseDataSchema.extend({
      transport: z.enum(['webrtc', 'relay', 'auto'])
    })
  }),
  baseMessageSchema.extend({
    type: z.literal('connection.reject'),
    data: deviceConnectionResponseDataSchema.extend({
      reason: reasonSchema
    })
  }),
  baseMessageSchema.extend({
    type: z.literal('signal.answer'),
    data: deviceConnectionResponseDataSchema.extend({
      sdp: sdpSchema
    })
  }),
  baseMessageSchema.extend({
    type: z.literal('signal.ice_candidate'),
    data: deviceConnectionResponseDataSchema.extend({
      candidate: candidateSchema,
      sdp_mid: z.string().min(1).max(160).nullable().optional(),
      sdp_mline_index: z.number().int().nonnegative().nullable().optional()
    })
  }),
  baseMessageSchema.extend({
    type: z.literal('signal.cancel'),
    data: deviceConnectionResponseDataSchema.extend({
      reason: reasonSchema
    })
  })
])

export const deviceSocketMessageSchema = z.discriminatedUnion('type', [
  deviceHelloMessageSchema,
  deviceHeartbeatMessageSchema,
  ...deviceResponseMessageSchema.options
])

export type DeviceSocketMessage = z.infer<typeof deviceSocketMessageSchema>
