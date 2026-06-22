import { resolve } from 'node:path'
import { defineConfig } from 'vite'

export default defineConfig({
  clearScreen: false,
  server: {
    strictPort: true,
    port: 58415,
    watch: {
      // 本地隔离 worktree 是完整嵌套项目，Vite 监听它会拖慢 dev 首屏并触发无关 reload。
      ignored: ['**/.worktrees/**', '**/target/**']
    }
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        eventCenter: resolve(__dirname, 'event-center.html')
      }
    }
  },
  envPrefix: ['VITE_', 'TAURI_']
})
