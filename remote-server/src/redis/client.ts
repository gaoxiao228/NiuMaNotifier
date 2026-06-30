import { Redis } from 'ioredis'

export function createRedis(redisUrl: string) {
  return new Redis(redisUrl, {
    lazyConnect: true,
    maxRetriesPerRequest: 2
  })
}
