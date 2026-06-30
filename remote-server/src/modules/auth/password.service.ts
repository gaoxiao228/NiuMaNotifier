import { hash, verify } from '@node-rs/argon2'

export type PasswordHashResult = {
  hash: string
  algo: 'argon2id'
}

export async function hashPassword(password: string): Promise<PasswordHashResult> {
  return {
    hash: await hash(password, {
      memoryCost: 19456,
      timeCost: 2,
      parallelism: 1
    }),
    algo: 'argon2id'
  }
}

export async function verifyPassword(passwordHash: string, password: string): Promise<boolean> {
  return verify(passwordHash, password)
}
