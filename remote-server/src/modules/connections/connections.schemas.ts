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

const sdpSchema = z.string().min(1).max(262144)
const candidateSchema = z.string().min(1).max(8192)

export const signalOfferPayloadSchema = z.object({
  sdp: sdpSchema
})

export const signalIceCandidatePayloadSchema = z.object({
  candidate: candidateSchema,
  sdp_mid: z.string().min(1).max(160).nullable().optional(),
  sdp_mline_index: z.number().int().nonnegative().nullable().optional()
})

export const clientSignalMessageSchema = z.discriminatedUnion('type', [
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.offer'),
    data: signalOfferPayloadSchema
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.ice_candidate'),
    data: signalIceCandidatePayloadSchema
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
