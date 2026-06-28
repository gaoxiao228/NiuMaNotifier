import { z } from 'zod'

export const connectionCreateSchema = z.object({
  device_id: z.string().min(1).max(160),
  client_id: z.string().min(1).max(160),
  transport_preference: z.enum(['webrtc_first', 'relay_first', 'relay_only']).default('webrtc_first')
})

export const connectionClientBindSchema = z.object({
  connection_id: z.string().min(1).max(160),
  connection_token: z.string().min(32)
})

export const signalingPayloadSchema = z.object({
  sdp: z.string().optional(),
  candidate: z.unknown().optional()
})

export const clientSignalMessageSchema = z.discriminatedUnion('type', [
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.offer'),
    data: signalingPayloadSchema
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.answer'),
    data: signalingPayloadSchema
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.ice_candidate'),
    data: signalingPayloadSchema
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.cancel'),
    data: z.object({ reason: z.string().min(1).max(240) })
  })
])

export type ConnectionCreateInput = z.infer<typeof connectionCreateSchema>
export type ConnectionClientBindInput = z.infer<typeof connectionClientBindSchema>
export type ClientSignalMessage = z.infer<typeof clientSignalMessageSchema>
