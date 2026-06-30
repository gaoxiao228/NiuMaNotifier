import { describe, expect, it } from 'vitest'
import { createBootstrapAdminService } from '../src/db/bootstrap-admin.js'
import { verifyPassword } from '../src/modules/auth/password.service.js'

function createFakeRepo() {
  const users: any[] = []

  return {
    users,
    async findAdmin() {
      return users.find((user) => user.role === 'admin') ?? null
    },
    async findUserByEmail(email: string) {
      return users.find((user) => user.email === email) ?? null
    },
    async createUser(input: any) {
      const user = { id: `usr_${users.length + 1}`, ...input }
      users.push(user)
      return user
    }
  }
}

describe('bootstrap admin service', () => {
  it('creates the first admin from bootstrap credentials', async () => {
    const repo = createFakeRepo()
    const service = createBootstrapAdminService({ repo })

    const result = await service.bootstrap({
      email: 'admin@example.com',
      password: 'password123',
      now: new Date('2026-06-30T00:00:00Z')
    })

    expect(result).toEqual({ created: true, skipped: false })
    expect(repo.users).toHaveLength(1)
    expect(repo.users[0].email).toBe('admin@example.com')
    expect(repo.users[0].role).toBe('admin')
    await expect(verifyPassword(repo.users[0].passwordHash, 'password123')).resolves.toBe(true)
  })

  it('does not create another admin when one already exists', async () => {
    const repo = createFakeRepo()
    repo.users.push({ email: 'admin@example.com', role: 'admin' })
    const service = createBootstrapAdminService({ repo })

    const result = await service.bootstrap({
      email: 'next-admin@example.com',
      password: 'password123',
      now: new Date('2026-06-30T00:00:00Z')
    })

    expect(result).toEqual({ created: false, skipped: true })
    expect(repo.users).toHaveLength(1)
  })

  it('rejects bootstrap when the email already belongs to a normal user', async () => {
    const repo = createFakeRepo()
    repo.users.push({ email: 'user@example.com', role: 'user' })
    const service = createBootstrapAdminService({ repo })

    await expect(
      service.bootstrap({
        email: 'user@example.com',
        password: 'password123',
        now: new Date('2026-06-30T00:00:00Z')
      })
    ).rejects.toThrow('BOOTSTRAP_ADMIN_EMAIL 已属于普通用户，不能自动提升为管理员')
  })
})
