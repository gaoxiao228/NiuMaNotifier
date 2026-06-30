import '@testing-library/jest-dom/vitest'

if (!window.matchMedia) {
  // Ant Design 的响应式 observer 在 jsdom 中需要浏览器 matchMedia API。
  window.matchMedia = (query) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false
  })
}
