import type { RpcFrame } from './types.js'

export type RemoteEncryptedTransport = {
  sendFrame(frame: RpcFrame): Promise<void>
  onFrame(callback: (frame: RpcFrame) => void): () => void
  close(reason: string): void
}
