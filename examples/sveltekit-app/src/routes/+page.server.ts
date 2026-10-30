export const load = async ({ url }) => {
  const name = url.searchParams.get('name') ?? 'world';
  const user = await embed.invoke<{ message: string }>('greet', { name });
  const hits = await embed.invoke<number>('hit');

  return { message: user.message, hits };
};
