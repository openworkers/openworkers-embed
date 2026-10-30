declare global {
  // Installed by openworkers-embed: calls the Rust command registered under `name`.
  const embed: {
    invoke<T = unknown>(name: string, args?: unknown): Promise<T>;
  };
}

export {};
