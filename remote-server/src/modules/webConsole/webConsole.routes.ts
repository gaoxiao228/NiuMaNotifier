import type { FastifyInstance, FastifyReply } from 'fastify'
import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { extname, join, normalize, resolve } from 'node:path'

export type WebConsoleRoutesOptions = {
  root?: string
}

function defaultWebConsoleRoot() {
  return resolve(process.cwd(), 'web/dist')
}

function contentTypeFor(filePath: string) {
  switch (extname(filePath)) {
    case '.js':
      return 'text/javascript; charset=utf-8'
    case '.css':
      return 'text/css; charset=utf-8'
    case '.svg':
      return 'image/svg+xml'
    case '.png':
      return 'image/png'
    case '.ico':
      return 'image/x-icon'
    default:
      return 'application/octet-stream'
  }
}

export async function registerWebConsoleRoutes(
  app: FastifyInstance,
  options: WebConsoleRoutesOptions = {}
) {
  const root = options.root ? resolve(options.root) : defaultWebConsoleRoot()

  if (!existsSync(join(root, 'index.html'))) {
    app.log.warn({ root }, 'web console dist not found; root page will use API 404 handler')
    return
  }

  const sendIndex = async (_request: unknown, reply: FastifyReply) => {
    const html = await readFile(join(root, 'index.html'), 'utf8')
    return reply.type('text/html; charset=utf-8').send(html)
  }

  app.get('/assets/*', async (request, reply) => {
    const wildcard = (request.params as { '*': string })['*']
    const relativePath = normalize(wildcard).replace(/^(\.\.(\/|\\|$))+/, '')
    const filePath = resolve(root, 'assets', relativePath)
    const assetsRoot = resolve(root, 'assets')
    if (!filePath.startsWith(`${assetsRoot}/`) && filePath !== assetsRoot) {
      return reply.status(404).send()
    }

    const content = await readFile(filePath)
    return reply.type(contentTypeFor(filePath)).send(content)
  })

  // React 控制台是单页应用，浏览器刷新内部路径时回退到 index.html。
  app.get('/', sendIndex)
  app.get('/console/*', sendIndex)
}
