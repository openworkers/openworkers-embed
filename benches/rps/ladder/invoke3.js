export default {
  async fetch(request, env) {
    let rows = 0;

    for (let i = 0; i < 3; i++) {
      rows += (await env.EMBED.invoke('query', { sql: 'select 1 as x', params: [] })).length;
    }

    return new Response(String(rows));
  },
};
