import { mkdir, mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { registerWebConsoleRoutes } from '../src/modules/webConsole/webConsole.routes.js'

describe('web console routes', () => {
  it('serves the web console html from root path', async () => {
    const root = await mkdtemp(join(tmpdir(), 'niuma-web-console-'))
    await mkdir(join(root, 'assets'))
    await writeFile(join(root, 'index.html'), '<!doctype html><title>NiuMaNotifier Remote Console</title>')
    const app = buildApp({
      registerWebConsoleRoutes: async (instance) => registerWebConsoleRoutes(instance, { root })
    })

    const response = await app.inject({ method: 'GET', url: '/' })

    expect(response.statusCode).toBe(200)
    expect(response.headers['content-type']).toContain('text/html')
    expect(response.body).toContain('NiuMaNotifier Remote Console')
  })

  it('serves built web console assets', async () => {
    const root = await mkdtemp(join(tmpdir(), 'niuma-web-console-'))
    await mkdir(join(root, 'assets'))
    await writeFile(join(root, 'index.html'), '<!doctype html>')
    await writeFile(join(root, 'assets', 'index-test.js'), 'console.log("remote console")')
    const app = buildApp({
      registerWebConsoleRoutes: async (instance) => registerWebConsoleRoutes(instance, { root })
    })

    const response = await app.inject({ method: 'GET', url: '/assets/index-test.js' })

    expect(response.statusCode).toBe(200)
    expect(response.headers['content-type']).toContain('text/javascript')
    expect(response.body).toContain('remote console')
  })
})
