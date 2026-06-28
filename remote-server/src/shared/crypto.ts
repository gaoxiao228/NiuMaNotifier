import { createHash as nodeCreateHash, randomBytes } from 'node:crypto'

export function createRandomToken(prefix: string): string {
  return `${prefix}_${randomBytes(32).toString('base64url')}`
}

export function createHash(value: string, pepper: string): string {
  return nodeCreateHash('sha256').update(`${pepper}:${value}`).digest('hex')
}
