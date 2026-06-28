import { CompactEncrypt, importJWK, type JWK } from 'jose'

export type DesktopLoginEncryptedResult = {
  alg: 'ECDH-ES+A256GCM'
  jwe: string
}

export async function encryptDesktopLoginResult(
  desktopPublicKeyJson: string,
  payload: object
): Promise<DesktopLoginEncryptedResult> {
  const publicJwk = JSON.parse(desktopPublicKeyJson) as JWK
  const publicKey = await importJWK(publicJwk, 'ECDH-ES')
  const plaintext = new TextEncoder().encode(JSON.stringify(payload))
  const jwe = await new CompactEncrypt(plaintext)
    .setProtectedHeader({ alg: 'ECDH-ES', enc: 'A256GCM' })
    .encrypt(publicKey)

  return {
    alg: 'ECDH-ES+A256GCM',
    jwe
  }
}
