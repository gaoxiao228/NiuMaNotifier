import { z } from 'zod'

export const relayBindSchema = z.object({
  connection_id: z.string().min(1).max(160),
  connection_token: z.string().min(32),
  side: z.enum(['client', 'device'])
})

export const relayFrameSchema = z.object({
  version: z.literal(1),
  type: z.literal('relay.frame'),
  id: z.string().min(1).max(160),
  connection_id: z.string().min(1).max(160),
  seq: z.number().int().positive(),
  ciphertext: z.string().min(1).max(1024 * 1024)
})

export type RelayBindInput = z.infer<typeof relayBindSchema>
export type RelayFrame = z.infer<typeof relayFrameSchema>
export type RelaySide = RelayBindInput['side']
