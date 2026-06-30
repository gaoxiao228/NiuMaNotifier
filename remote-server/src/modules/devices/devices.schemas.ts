import { z } from 'zod'
import { deviceCapabilitiesSchema } from '../desktopLogin/desktopLogin.schemas.js'

export const deviceIdSchema = z.string().min(1).max(160)

export const deviceRenameSchema = z.object({
  device_id: deviceIdSchema,
  name: z.string().min(1).max(120)
})

export const deviceRegisterSchema = z.object({
  device_name: z.string().min(1).max(120),
  device_fingerprint: z.string().min(32).max(128),
  capabilities: deviceCapabilitiesSchema
})

export const deviceRevokeTokenSchema = z.object({
  device_id: deviceIdSchema
})
