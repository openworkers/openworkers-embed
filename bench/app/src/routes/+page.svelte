<script lang="ts">
  let { data } = $props();
</script>

<svelte:head>
  <title>Rock, paper, scissors</title>
</svelte:head>

<h1>Rock, paper, scissors</h1>

<p>
  {data.totals.games} games: {data.totals.wins} won, {data.totals.losses} lost,
  {data.totals.draws} drawn.
</p>

<h2>Leaders</h2>
<table>
  <thead><tr><th>Player</th><th>Wins</th><th>Losses</th><th>Draws</th></tr></thead>
  <tbody>
    {#each data.leaders as leader (leader.name)}
      <tr><td>{leader.name}</td><td>{leader.wins}</td><td>{leader.losses}</td><td>{leader.draws}</td></tr>
    {/each}
  </tbody>
</table>

<h2>Last games</h2>
<ul>
  {#each data.recent as game (game.id)}
    <li class={game.result}>{game.name} played {game.player} against {game.server}: {game.result}</li>
  {/each}
</ul>
