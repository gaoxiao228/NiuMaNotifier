import { randomBytes } from 'node:crypto'

export function createPublicId(prefix: string): string {
  return `${prefix}_${randomBytes(16).toString('base64url')}`
}
