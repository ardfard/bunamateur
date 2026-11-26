const CACHE_VERSION = 'bunamateur-v1'
const APP_SHELL = ['/', '/manifest.webmanifest', '/icon.svg']

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE_VERSION)
      .then((cache) => cache.addAll(APP_SHELL))
      .then(() => self.skipWaiting())
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== CACHE_VERSION).map((k) => caches.delete(k)))
      )
      .then(() => self.clients.claim())
  )
})

self.addEventListener('fetch', (event) => {
  const { request } = event
  if (request.method !== 'GET') return

  const url = new URL(request.url)
  if (url.origin !== self.location.origin) return

  // Never cache API calls (auth, reviews, progress mutations, etc.)
  if (url.pathname.startsWith('/api/')) return

  // Cache-first for Vite hashed assets
  if (url.pathname.startsWith('/assets/')) {
    event.respondWith(
      caches.open(CACHE_VERSION).then(async (cache) => {
        const cached = await cache.match(request)
        if (cached) return cached
        const response = await fetch(request)
        if (response.ok) {
          cache.put(request, response.clone())
        }
        return response
      })
    )
    return
  }

  // Network-first with cached app shell fallback for SPA navigations
  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok && url.pathname === '/') {
            const copy = response.clone()
            caches.open(CACHE_VERSION).then((cache) => cache.put('/', copy))
          }
          return response
        })
        .catch(async () => {
          const cache = await caches.open(CACHE_VERSION)
          return (await cache.match('/')) || Response.error()
        })
    )
    return
  }

  // Cache-first for static shell assets (icon, manifest)
  if (APP_SHELL.includes(url.pathname)) {
    event.respondWith(
      caches.open(CACHE_VERSION).then(async (cache) => {
        const cached = await cache.match(request)
        if (cached) return cached
        const response = await fetch(request)
        if (response.ok) {
          cache.put(request, response.clone())
        }
        return response
      })
    )
  }
})
