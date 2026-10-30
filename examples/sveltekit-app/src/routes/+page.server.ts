export const load = async ({ url, platform }) => {
  const name = url.searchParams.get('name') ?? 'world';
  const user = await platform!.invoke<{ message: string }>('greet', { name });
  const hits = await platform!.invoke<number>('hit');

  return { message: user.message, hits };
};
