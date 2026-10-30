if (globalThis.default && typeof globalThis.default.fetch === 'function') {
    const handler = globalThis.default;

    addEventListener('fetch', (event) => {
        event.respondWith(
            handler.fetch(event.request, globalThis.env, {
                waitUntil: (promise) => event.waitUntil(promise),
                passThroughOnException() {},
            }),
        );
    });
}
