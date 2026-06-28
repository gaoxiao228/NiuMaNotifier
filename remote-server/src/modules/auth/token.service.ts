import { importPKCS8, importSPKI, jwtVerify, SignJWT } from 'jose'

export type AccessTokenInput = {
  userId: string
  sessionId: string
  role: 'admin' | 'user'
}

export type AccessTokenConfig = {
  privateKeyPem: string
  ttlSeconds: number
}

export async function createAccessToken(
  input: AccessTokenInput,
  config: AccessTokenConfig
): Promise<string> {
  const privateKey = await importPKCS8(config.privateKeyPem, 'EdDSA')

  return new SignJWT({
    session_id: input.sessionId,
    role: input.role
  })
    .setProtectedHeader({ alg: 'EdDSA' })
    .setSubject(input.userId)
    .setIssuedAt()
    .setExpirationTime(`${config.ttlSeconds}s`)
    .sign(privateKey)
}

export async function verifyAccessToken(token: string, publicKeyPem: string) {
  const publicKey = await importSPKI(publicKeyPem, 'EdDSA')
  const { payload } = await jwtVerify(token, publicKey)

  return {
    userId: String(payload.sub),
    sessionId: String(payload.session_id),
    role: payload.role === 'admin' ? 'admin' : 'user'
  }
}
