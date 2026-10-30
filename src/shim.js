(function () {
    const host = env.__embed;

    globalThis.embed = {
        invoke: (name, args) => host.invoke(name, args),
    };
})();
