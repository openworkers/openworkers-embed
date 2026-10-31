export type Leader = { name: string; wins: number; losses: number; draws: number };
export type Game = { id: number; name: string; player: string; server: string; result: string };
export type Totals = { games: number; wins: number; losses: number; draws: number };

/// The three reads of the page, one command each
export async function board(platform: App.Platform) {
  const leaders = await platform.invoke<Leader[]>('leaders');
  const recent = await platform.invoke<Game[]>('recent');
  const totals = await platform.invoke<Totals>('totals');

  return { leaders, recent, totals };
}
