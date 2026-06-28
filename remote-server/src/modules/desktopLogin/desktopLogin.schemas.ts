import { z } from 'zod'

export const deviceCapabilitiesSchema = z.object({
  agent_protocol_version: z.number().int().positive(),
  rpc_protocol_version: z.number().int().positive(),
  supports_webrtc: z.boolean(),
  supports_relay: z.boolean(),
  supports_remote_control: z.boolean()
})

export const desktopLoginStartSchema = z.object({
  device_name: z.string().min(1).max(120),
  device_fingerprint: z.string().min(32).max(128),
  desktop_public_key: z.string().min(16),
  capabilities: deviceCapabilitiesSchema
})

export const desktopLoginCompleteSchema = z.object({
  request_id: z.string().min(1).max(160)
})

export const desktopLoginPollSchema = z.object({
  request_id: z.string().min(1).max(160),
  poll_token: z.string().min(32)
})

export type DesktopLoginStartInput = z.infer<typeof desktopLoginStartSchema>
export type DesktopLoginCompleteInput = z.infer<typeof desktopLoginCompleteSchema>
export type DesktopLoginPollInput = z.infer<typeof desktopLoginPollSchema>
