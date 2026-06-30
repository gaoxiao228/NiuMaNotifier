import { createHash, createRandomToken } from '../../shared/crypto.js'

export function createConnectionTokenService(options: { tokenPepper: string }) {
  return {
    issue() {
      const token = createRandomToken('cnt')

      return {
        token,
        tokenHash: createHash(token, options.tokenPepper)
      }
    },
    verify(token: string, expectedHash: string) {
      return createHash(token, options.tokenPepper) === expectedHash
    }
  }
}
