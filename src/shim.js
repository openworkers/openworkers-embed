globalThis.embed = {
    async invoke(name, args) {
        const response = await env.__embed.fetch(`http://embed/${name}`, {
            method: 'POST',
            body: JSON.stringify(args ?? null),
        });

        if (!response.ok) {
            throw new Error(await response.text());
        }

        return response.json();
    },
};
