<script lang="ts">
  let { data } = $props();
</script>

<h1>{data.message}</h1>
<p>Server-rendered by SvelteKit in V8, hit {data.hits}.</p>
