import { z } from 'zod'

export const emailSchema = z.string().email()
export const passwordSchema = z.string().min(8).max(128)

export const authRegisterSchema = z.object({
  email: emailSchema,
  password: passwordSchema
})

export const authLoginSchema = z.object({
  email: emailSchema,
  password: passwordSchema
})

export const authRefreshSchema = z.object({
  refresh_token: z.string().min(32)
})

export const authLogoutSchema = z.object({
  refresh_token: z.string().min(32)
})

export type AuthRegisterInput = z.infer<typeof authRegisterSchema>
export type AuthLoginInput = z.infer<typeof authLoginSchema>
export type AuthRefreshInput = z.infer<typeof authRefreshSchema>
export type AuthLogoutInput = z.infer<typeof authLogoutSchema>
